//! Organon — VST3/CLAP plugin + standalone.
//!
//! The plugin is a thin *control surface*: it declares every parameter so the
//! host (Ableton) maps/automates them natively, and its editor is just sliders.
//! The GPU visual lives in a separate top-level window (next milestone) that
//! reads these same parameter values in-process.

// #572 (the world hoist) — `render.rs` refers to `organic_math_native::math::…` by crate name.
// That spelling is what lets the file be byte-identical between its two hosts: the binary,
// where `organic_math_native` is an external crate, and this library, where without this line
// the name would not resolve at all. One line here beats forking a 6 000-line file.
extern crate self as organic_math_native;

pub mod agent;
pub mod audio;
pub mod cli;
pub mod clip;
pub mod console_catalog;
mod controller;

/// #626 Tier 3 — **`organon-core`'s modules, re-exported so `crate::` paths still resolve.**
///
/// `edition`, `gguf` and `gguf_data` now live in `native/organon-core`, a crate whose
/// `[dependencies]` is empty (no `nih_plug`, no `wgpu`, no `egui` — `cargo tree -p
/// organon-core` is the tier's acceptance test). These three lines are a **facade**: every
/// existing `crate::edition::EDITION` / `crate::gguf::parse_file` path in the crate keeps
/// working, so the move is a crate boundary rather than a ~60-site rewrite.
///
/// **Why a facade instead of rewriting the call sites to `organon_core::`** — the honest
/// reason, so it is not mistaken for laziness:
///
/// 1. **`gguf`/`gguf_data` move AGAIN in Tier 4**, out of core and into `organon-mind`,
///    once the lens builders leave `math.rs` (#536 T4 reference #1). Rewriting 19
///    `crate::gguf` sites to `organon_core::gguf` now and to `organon_mind::gguf` next
///    tier is the same churn paid twice.
/// 2. **The claim this tier makes is behavioural identity**, and the smallest diff that
///    achieves the boundary is the one most likely to hold it. A GPU-less host cannot run
///    `verify.sh`, so diff size *is* part of the argument.
///
/// **Named, never glob.** `pub use organon_core::*` would let core silently widen this
/// crate's surface later; these are three named modules and nothing else. `ARCHITECTURE.md`
/// §19 records the decision.
pub use organon_core::{edition, gguf, gguf_data, ipc, math};

/// #626 Tier 4 — the Mind cluster is its own crate now (`organon-mind`, no nih-plug).
/// Re-exported so every existing `crate::mind_ring::…` path in this crate still resolves,
/// the same facade Tier 3 used for core. `mind_main.rs` stays here: it is the
/// `organon-mind` **binary** and it needs nih-plug's standalone wrapper.
pub use organon_mind::{mind_console, mind_log, mind_ring, mind_shell, mind_ui, mind_viz};

/// organon#49 Tier 3 — the **substrate** is its own crate now (`organon-scene`: no
/// nih-plug, no wgpu, no egui). Re-exported so every existing
/// `crate::substrate_scene::…` / `crate::overlay_meta::…` path in this crate still
/// resolves — the same facade Tier 3 used for core and Tier 4 for Mind.
///
/// ⚠️ **`scene_input` is NOT in this list and stays in this crate.** It is the sibling
/// that looks like it belongs: same Console Spike lineage, same `substrate` subject. But
/// it reaches `egui` (`Pos2`/`Rect`/`Context`/`RawInput`) to turn pointer gestures into
/// `CameraInput`, and `organon-scene`'s claim is that it has no UI toolkit. It travels
/// with `world.rs` in Tier 4.
///
/// **Named, never glob**, for the reason core's re-export gives one screen up.
pub use organon_scene::{
    overlay_meta, substrate_camera, substrate_epochs, substrate_materials, substrate_scene,
};

/// organon#49 Tier 4b — the **window layer** is its own crate now (`organon-world`: egui
/// and wgpu on purpose, **no nih-plug**). Re-exported so every existing
/// `crate::scene_input::…` / `crate::frame_ring::…` path in this crate still resolves —
/// the same facade Tier 3 used for core, Tier 4 for Mind and T3 for the substrate.
///
/// ⚠️ **`agent` and `cli` are deliberately NOT in this list**, though `world.rs` imports
/// them alongside these five. They carry `param_table` and `preset` — the plugin's own
/// automation surface — so they are host-side until something changes that, and Tier 4c
/// has to answer them rather than widen the crate to swallow them.
///
/// **Named, never glob**, for the reason core's re-export gives above.
pub use organon_world::{audio_ring, egui_platform, scene_input};

/// #554 Tier 1 — the **frame mirror**: the visual's rendered frames carried to the editor
/// over their own mmap, so the editor can draw a live viewport inside its own window.
/// Separate from `Shared` (high-rate payload); see the module docs for why the boundary is
/// CPU memory rather than a shared GPU texture.
///
/// **#593 Tier 4 gated it out of Organon Mind, and gating is all it did.** The mirror is full
/// Organon's *only* viewport path — inside Ableton the editor does not own its window and a GPU
/// device must not enter the host's process — so deleting it, which #593's original text asked
/// for, would delete the shipping plugin's viewport. In Mind the wgpu editor renders the world
/// into the editor's own surface instead, and this whole subsystem has nothing left to do; the
/// mind-edition build **not compiling** if anything on that path still names it is the tier's
/// completion test. `MIND_ARCHITECTURE.md` §2.5 carries the per-item verdict.
///
/// ⚠️ **The gate is on the RE-EXPORT, and it has to be** (organon#49 T4b). `frame_ring`
/// moved to `organon-world`, and a name inside a braced `pub use` list cannot be
/// cfg'd — so it gets its own statement. Dropping the gate would silently hand Mind back
/// a subsystem #593 T4 removed from it, and #593's completion test is precisely that a
/// mind-edition build fails to compile if anything on that path still *names* it. That
/// property is preserved: `crate::frame_ring` does not resolve under `mind-edition`.
/// (`organon-world` still compiles the module itself either way — it is mmap plumbing
/// with no side effects — but nothing on Mind's path can name it.)
#[cfg(not(feature = "mind-edition"))]
pub use organon_world::frame_ring;
/// #593 Tier 3 — **the baseview arm** of the `EguiPlatform` seam, the counterpart to
/// `winit_platform.rs`.
///
/// ⚠️ **It lives in the library, not beside `winit_platform` inside `world.rs`'s `#[path]`
/// tree, and that is forced by the orphan rule** — see the module docs. Ungated for the same
/// reason `baseview_input` is: it is a pure adapter, and gating it would drop its tests out of
/// a default `cargo test`.
pub mod baseview_platform;
/// #593 Tier 0 — **the route-C probe**: our own `nih_plug::editor::Editor` that builds a
/// `wgpu::Surface` on the parent view the host hands `spawn`, clears it to a *cycling*
/// colour and presents. It is the compiled form of the one claim the whole #593 thread
/// rests on, and the skeleton Tier 2 grows into — not a spike.
///
/// Gated on `mind-edition` so the shipping plugin cdylib cannot move, and armed at runtime
/// by `ORGANON_EDITOR_PROBE=1` (checked at the top of `editor()` below). See the module
/// docs for the handle chain and for what only the Mac can settle.
#[cfg(feature = "mind-edition")]
pub mod editor_probe;
/// #599 — baseview events → `egui::RawInput`, the winit-free replacement for `egui-winit`
/// on the editor path (where the window is an `NSView` nih-plug hands us and there is no
/// winit in the process). Mirrors `egui_winit::State`'s four call sites one-for-one.
///
/// **Ungated on purpose.** #593 Tier 2 is its first consumer and that is `mind-edition`-only,
/// but gating this too would drop its 78 tests out of a default `cargo test` — the suite would
/// silently shrink by exactly the coverage #599 added. It is pure translation (no wgpu, no
/// world), so the size it costs an ungated build is small; the cdylib is measured in the PR.
pub mod baseview_input;
/// #593 Tier 2 — **the custom wgpu `Editor`**: `World::render_into` plus the real interface,
/// on one device, in the window nih-plug hands us. Gated on `mind-edition` (so the shipping
/// plugin cdylib cannot move), and **Organon Mind's editor by default** since #593 closed;
/// `ORGANON_EDITOR_WGPU=0` is the bring-up fallback, checked in `editor()` below. See the
/// module docs for what only the Mac can settle.
#[cfg(feature = "mind-edition")]
pub mod wgpu_editor;
mod keymap;
pub mod material_graph;

pub mod param_table;
pub mod params;
mod preset;
pub mod recipe;
pub mod synth;
/// #542 Tier 1 — the house style: design tokens, the egui theme, and the control-row
/// grid. Everything that decides how the editor *looks* resolves here rather than being
/// scattered across `lib.rs` as colour literals and a 12-line `apply_theme`.
pub mod theme;
/// #551 Tier 1 — the UI theme as **runtime state**: every colour and material treatment
/// `theme.rs` paints with, made configurable, persisted to its own JSON store, and edited live
/// from the `UI` panel. Deliberately *not* nih-plug params — recalling a Scene must never
/// restyle the editor.
pub mod theme_config;
// #572 route C — the renderer as a LIBRARY module tree, so the editor can reach it.
// See `world.rs` for why it exists and what still has to move into it.
//
// **Gated, and the gate was chosen by measurement rather than caution.** Ungated, this module
// grows the plugin cdylib from 12 749 728 to 13 250 704 bytes (+490 KB) — it exports no wgpu or
// naga dynamic symbols either way, so nothing new is *reachable*, but a shipping VST3 that
// changes size for no user-visible reason is exactly what "full Organon is untouched" rules out.
// Organon Mind ships no VST3 (#483), so under `mind-edition` the growth costs nothing.
//
// Shell #6 T1 widened the gate to `shell-edition` — Shell's embedded viewport is a
// third consumer of the world, and like Mind it ships no plugin, so the cdylib measurement
// above still holds for the only build that has one (the default, where both features are
// off and this module still does not exist).
#[cfg(any(feature = "mind-edition", feature = "shell-edition"))]
pub mod world;
/// #520 Tier 2 — making the **standalone**'s window resizable. baseview opens it
/// with no `Resizable` style bit and offers no API to change that, so this reaches
/// the `NSWindow` through objc, the way `hdr_macos.rs` reaches wgpu's
/// `CAMetalLayer`. Inert in the plugin (the host owns that window) and off macOS.
pub mod window_macos;

use arc_swap::ArcSwap;
use nih_plug::prelude::*;
use nih_plug_egui::resizable_window::ResizableWindow;
use nih_plug_egui::widgets::ParamSlider;
use nih_plug_egui::{create_egui_editor, egui};
use params::OrganicMathParams;
// #542 Tier 1 — the row-grid tokens live in `theme` beside the palette they're laid out
// against. Imported under their old names so the ~50 `2.0 * COMBO_W` call sites and the
// `value_box` reserve read exactly as before.
use theme::{COMBO_W, VALUE_W};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

/// Sentinel for `active_note`: no mapped key is currently held.
const NO_NOTE: u8 = 255;

pub struct OrganicMath {
    params: Arc<OrganicMathParams>,
    /// Shared-memory channel feeding the separate visual process.
    visual_writer: Option<ipc::Writer>,
    /// Latest normalized value (0..1) received per parameter CC. These override
    /// the visual snapshot — a played/dropped MIDI clip drives the look.
    cc_override: [Option<f32>; clip::N],
    /// Each parameter's normalized value last frame. When it changes, the user
    /// (or host automation) moved that control, so we release its CC override —
    /// "last touched wins", and the sliders always win back from a clip.
    last_norm: [f32; clip::N],
    /// Set by the editor's "Release MIDI clip" button; clears all CC overrides.
    release: Arc<AtomicBool>,
    /// Bumped by the editor's "Open HDR Environment…" button after it writes the
    /// chosen path to the sidecar. `process()` copies it into the IPC snapshot;
    /// the visual watches it and re-runs IBL precompute (mirrors `release`).
    hdr_gen: Arc<AtomicU32>,
    /// #472 Tier 1 — bumped by the editor's "Load Material…" button after it writes
    /// the chosen material FOLDER path to the material sidecar. `process()` copies it
    /// into `Shared.material_gen`; the visual watches it and (re)loads the PNG
    /// channel maps (mirrors `hdr_gen`).
    material_gen: Arc<AtomicU32>,
    /// #354: the live host beat position (absolute beats) as `f32` bits, written
    /// by `process()` and read by the editor's GUI thread to fire beat-quantized
    /// preset recalls. `-1.0` = transport not playing (recall applies at once).
    beat_pos: Arc<AtomicU32>,
    /// #354: liveness flag for the **HDR responder** background thread — spawned
    /// once in `initialize()`, it watches `active_note` and makes a MIDI-held
    /// Scene preset's captured `.hdr` follow (sidecar write + `hdr_gen` bump, off
    /// the audio thread; the visual can't take file I/O on `process()`). `false`
    /// = not yet started / stop requested (`Drop` sets it so the thread exits).
    hdr_responder: Arc<AtomicBool>,
    /// Bumped by the editor when the overlay string sidecar (custom title / handle) is
    /// rewritten; `process()` copies it into the snapshot, the visual edge-detects it
    /// and re-reads the sidecar (#135 P2; mirrors `hdr_gen`).
    overlay_gen: Arc<AtomicU32>,
    /// Bumped by the Liquid card's "reset pool" button (#182 T3a); stamped into
    /// `Shared.liquid[14]` in `process()` (mirrors `hdr_gen` — a live counter,
    /// not a param, never preset-captured). The visual reseeds the pool on change.
    liq_reset_gen: Arc<AtomicU32>,
    /// Bumped by the Storyboard card's "next shot ▶" button (#307 Tier 3). `process()`
    /// stamps it into `Shared.cam_story[4]`; the visual edge-detects it and advances
    /// to the next storyboard shot at the next bar (mirrors `hdr_gen`; live counter,
    /// not a param).
    story_next_gen: Arc<AtomicU32>,
    /// Bumped by the Neural Network card's "Load Connectome…" button (#226 Tier 3)
    /// after it writes the chosen JSON path to the connectome sidecar. `process()`
    /// stamps it into `Shared.nn_gen`; the visual edge-detects it and ingests the
    /// file (mirrors `hdr_gen`).
    nn_gen: Arc<AtomicU32>,
    /// Bumped by the Creature Engine card's "Load Creature (JSON)…" button (#476
    /// Tier 2b) after it writes the chosen JSON path to the creature sidecar.
    /// `process()` stamps it into `Shared.creature_gen`; the visual edge-detects it
    /// and rebuilds the body plan (mirrors `nn_gen`).
    creature_gen: Arc<AtomicU32>,
    /// AI-Performer (#317 Tier 1) editor-thread atomics stamped into `Shared.agent[8]`
    /// by `process()` (the `hdr_gen`/`nn_gen` pattern; a runtime block, not params).
    /// `agent_on` = the Mind card has been engaged; `chat_gen` bumps when the editor
    /// writes the chat sidecar (`organic-math-chat.txt`); `plan_gen` bumps when a
    /// phrase-plan is written (`organic-math-plan.txt`); `release_gen` bumps on "Release
    /// agent" (the visual clears all agent holds). The agent RUNTIME lives in the visual.
    agent_on: Arc<AtomicBool>,
    chat_gen: Arc<AtomicU32>,
    plan_gen: Arc<AtomicU32>,
    release_gen: Arc<AtomicU32>,
    /// Intelligent preset names (#425): bumped when the editor saves a preset with
    /// auto-naming on (after writing the scene identity to `organic-math-namereq.txt`).
    /// `process()` stamps it into `Shared.agent[4]`; the visual edge-detects it, asks the
    /// local model for a name, and writes `organic-math-namereply.txt`.
    name_gen: Arc<AtomicU32>,
    /// Bumped by the Mind card's "Load AI Model… (.gguf)" button (#367 Tier 1)
    /// after it writes the chosen `.gguf` path to the model sidecar. `process()`
    /// stamps it into `Shared.mind[1]` (`model_gen`); the visual edge-detects it,
    /// parses the GGUF header, and builds the specimen topology (mirrors `nn_gen`).
    model_gen: Arc<AtomicU32>,
    /// Human-readable parsed-header readout (layers/heads/dims/vocab/tensors) the
    /// picker thread fills after parsing the chosen `.gguf`, shown in the Mind card.
    model_readout: Arc<std::sync::Mutex<String>>,
    /// Mind card **topology** selector (`topo_mode`). GUI-thread atomic; `process()`
    /// stamps it into the reserved `Shared.mind[2]` slot: **0** = the Tier-1 architecture
    /// specimen (default), **1** = Live streaming (#367 Tier 2 — the visual reads the
    /// activation ring `organic-math-mind.bin` and streams per-token activations into the
    /// connectome node-glow), **2** = the #507 Tier 1 **embedding galaxy** (the vocabulary
    /// embedding matrix projected to 3-D). No `Shared` size/LAYOUT_VERSION change — all
    /// three ride the slot `mind[2]` already occupied.
    mind_topo: Arc<AtomicU32>,
    /// #367 Tier 2b (embedded runtime) editor→runtime dials, stamped into the reserved
    /// `Shared.mind[3..8]` slots by `process()` (a runtime block, not params — the
    /// `hdr_gen`/`nn_gen` pattern). `mind_prompt_gen` bumps when the Mind card's
    /// "Generate" writes the prompt sidecar (`organic-math-mind-prompt.txt`); the
    /// optional `organic-math-mind-runtime` bin edge-detects it and runs one completion.
    /// The three float dials are stored as `f32` bits in `AtomicU32`.
    mind_prompt_gen: Arc<AtomicU32>,
    mind_temp: Arc<AtomicU32>, // f32 bits — sampling temperature
    mind_ctx: Arc<AtomicU32>,  // context length (tokens)
    mind_rate: Arc<AtomicU32>, // f32 bits — token-rate cap (tokens/sec, 0 = uncapped)
    mind_fullattn: Arc<AtomicU32>, // 0/1 — flash-attention OFF (per-head tap)
    /// #554 Tier 1 — is the **embedded viewport** on? `0`/`1`, stamped by `process()` into
    /// `Shared.mindview[3]` (a slot #541 T1 reserved, so no `LAYOUT_VERSION` movement).
    /// Runtime state, not a param: a viewport is a workspace arrangement, so host automation
    /// of it would be meaningless and a Scene recall must not open or close it.
    ///
    /// #593 Tier 4 — gone from the Mind edition along with the pane that sets it. The `Shared`
    /// slot itself is untouched and stays **reserved**: removing a field would be a
    /// `LAYOUT_VERSION` bump plus a golden re-pin against every saved Ableton set, for four
    /// bytes. In Mind it simply reads `0.0` forever, which is what it already means.
    #[cfg(not(feature = "mind-edition"))]
    viewport_on: Arc<AtomicU32>,
    /// #367 Tier 2 UX — the in-plugin **Mind console**: the plugin spawns the
    /// embedded `organic-math-mind-runtime` as a managed child (piped stdio) and
    /// the Mind card shows its log + a command REPL. Lives behind `Arc<Mutex<_>>`
    /// so the runtime survives editor close/reopen (see `mind_console.rs`).
    mind_console: Arc<std::sync::Mutex<mind_console::MindConsole>>,
    /// #423 Tier 1 — the atlas (design space). GUI-thread atomics, stamped by
    /// `process()` into the runtime block `Shared.atlas[0..3]` (`[atlas_gen, on,
    /// roofline_on]`; the `mind`/`model_gen` pattern, not params). `atlas_gen` bumps
    /// when the Design Space card's "Scan Model Library…" finishes writing the
    /// derived design points to `organic-math-atlas.json`; the visual edge-detects
    /// it, builds the constellation into `neural_loaded`, and (when `atlas_on`) shows
    /// the roofline inset (when `atlas_roofline`).
    atlas_gen: Arc<AtomicU32>,
    atlas_on: Arc<AtomicU32>,       // 0/1 — atlas active (draw constellation + roofline)
    atlas_roofline: Arc<AtomicU32>, // 0/1 — draw the roofline overlay inset
    /// Human-readable scan summary (N models, hardware profile, context) the scan
    /// thread fills, shown in the Design Space card.
    atlas_readout: Arc<std::sync::Mutex<String>>,
    /// A hardware profile parsed by the "Load Hardware Profile…" background thread,
    /// handed to the GUI thread (which adopts it into `PresetUi.atlas_custom_profile`
    /// on the next repaint, then clears it). `None` = nothing pending.
    atlas_loaded_profile: Arc<std::sync::Mutex<Option<crate::math::HardwareProfile>>>,
    /// Bumped by the Field Engine card's "Load Field Program…" / "Apply" buttons
    /// (#381 Tier 1) after the editor writes the program text to the field sidecar.
    /// `process()` stamps it into `Shared.field_gen`; the visual edge-detects it and
    /// recompiles the program (mirrors `nn_gen`).
    field_gen: Arc<AtomicU32>,
    /// Bumped by the Field Engine card's "Load NCA Model (JSON)…" button (Tier B,
    /// #407) after the editor writes the chosen weights-JSON path to the NCA sidecar.
    /// `process()` stamps it into `Shared.nca_gen`; the visual edge-detects it and
    /// (re)loads `math::NcaWeights` (mirrors `nn_gen`/`field_gen`).
    nca_gen: Arc<AtomicU32>,
    /// Set by the async "Load Field Program" dialog thread AFTER it writes the
    /// sidecar + bumps `field_gen`; the GUI thread consumes it to switch
    /// `field_preset` to Custom. Deferring the switch this way (vs. on click) avoids
    /// the visual re-reading a stale sidecar while the file dialog is still open.
    field_load_pending: Arc<AtomicBool>,
    /// Bumped by the Field Engine card's "Load Field Clip…" button (#407 Tier A) after
    /// it writes the chosen `.bin` path to `field_clip_sidecar_path()`. `process()`
    /// stamps it into `Shared.fieldclip_gen`; the visual edge-detects it and (re)loads
    /// the baked `math::FieldClip` (mirrors `field_gen`/`nn_gen`). Runtime block, not a param.
    fieldclip_gen: Arc<AtomicU32>,
    /// Audio-reactive band analyzer, created in `initialize` once the sample rate
    /// is known. `None` until then (and the visual just sees zero bands).
    analyzer: Option<audio::Analyzer>,
    /// Calibrated loudness/true-peak meter (#333 Tier 1). `None` until `initialize`.
    loudness: Option<audio::LoudnessMeter>,
    /// Calibrated fractional-octave RTA (#333 Tier 2). `None` until `initialize`.
    cal_spectrum: Option<audio::CalibratedSpectrum>,
    /// Live analysis published from `process()` for the editor's Audio panel
    /// (level meter + spectrum + per-band meters). Lock-free; read on the GUI thread.
    audio_viz: Arc<audio::AudioViz>,
    /// Raw-waveform ring for the oscilloscope (#333). Written every block; read by
    /// the editor's scope. Always captured (independent of Audio Reactive).
    scope: Arc<audio::ScopeRing>,
    /// Preallocated read buffer for the #346 Field Chamber wall-scope publish (so
    /// `process()` builds `Shared.scopewave` without an audio-thread allocation).
    scope_win: Vec<f32>,
    /// Host sample rate, captured in `initialize` (process() has no buffer config).
    sample_rate: f32,
    /// #430 audio-sample ring: the live post-synth stereo output streamed to the
    /// visual's in-app recorder so a recording can carry the music. Created in
    /// `initialize` with the host sample rate; written every block (audio output is
    /// byte-identical). `None` if the ring file couldn't be opened.
    audio_ring: Option<audio_ring::AudioRingWriter>,
    /// Key→preset map, resolved to per-note `Shared` snapshots. Edited on the GUI
    /// thread (the Key Map window) and published here; read wait-free in
    /// `process()` so a held MIDI note can drive the look. Loaded from disk at
    /// construction so keys work even before the editor is opened.
    keymap: Arc<ArcSwap<keymap::KeyMap>>,
    /// MIDI notes currently held (audio-thread only); the newest mapped one wins.
    held: keymap::HeldKeys,
    /// The mapped note currently driving the look (or `NO_NOTE`). Published for the
    /// editor to highlight the live key.
    active_note: Arc<AtomicU8>,
    /// #356 Four-Quadrant Performance Controller: a wait-free ring the audio
    /// thread fills with raw MIDI from the pad surface (when `perf_enable` is on)
    /// and the editor drains each repaint. The quantized recall it triggers is a
    /// GUI-thread (`ParamSetter`) path, so — unlike the Key Map — the pad press
    /// can't drive the visual directly; it rides this mailbox to the editor.
    perf_mailbox: Arc<controller::Mailbox>,
    /// Seqlock generation for atomic preset recall. The editor bumps it **odd**
    /// before mutating params (in `apply()`) and **even** after; `process()` only
    /// publishes a snapshot it captured wholly outside an apply, so the visual
    /// never sees a half-applied state (shape updated, colour not yet). Edited on
    /// the GUI thread, read wait-free in `process()`.
    apply_gen: Arc<AtomicU32>,
    /// #339 Duo-Field synthesis engine (Tier 1: field probes + played voices).
    /// Preallocated (audio-thread rule); renders the synth bus into the passthrough
    /// when `sn_on`. Audio-thread only.
    synth: synth::SynthEngine,
    /// Current pitch-bend in semitones (updated by MIDI pitch-bend events, baked
    /// into each block's voice tuning). Audio-thread only.
    synth_bend: f32,
    /// #339 item 1 — the synth's own beat accumulator (beats elapsed). Synced to
    /// host `pos_beats` while the transport plays, else free-run from the manual
    /// tempo, so the beat pump + cavity mode-walk move the SOUND generatively (no
    /// audio needed) and stay beat-aligned to the visual (both lock the same host
    /// clock). Audio-thread only.
    synth_beat: f64,
    /// Last integer beat (#339 Tier 3) — a change is a beat crossing = a mallet
    /// strike for the modal bank. Audio-thread only.
    synth_beat_floor: i64,
    /// Previous block's RMS level (#339 Tier 3) — a rising edge is an input
    /// transient that strikes the modal cavity. Audio-thread only.
    synth_prev_level: f32,
    /// The active beat-clock BPM (f32 bits) the synth uses this block, published
    /// lock-free for the editor's perf-footer tempo readout.
    active_bpm: Arc<AtomicU32>,
    /// The active tempo source (0 Host / 1 Audio / 2 Manual), for the same readout.
    tempo_src_active: Arc<AtomicU32>,
    /// Last valid host tempo (BPM) seen from the transport — held so Host mode keeps
    /// following the DAW tempo across blocks where the host momentarily omits it
    /// (some hosts drop it when the transport is stopped). Audio-thread only.
    last_host_bpm: f64,
    /// Last host `pos_beats` — the synth beat only snaps to the host grid when this
    /// is actually advancing (else it free-runs at the resolved BPM), so a host that
    /// withholds `pos_beats` from the effect can't stall the beat. Audio-thread only.
    last_pos_beats: f64,
    /// Last `apply_gen` seen — a change (preset recall completed) damps the modal
    /// ring so it doesn't bleed into the new patch. Audio-thread only.
    last_apply_gen: u32,
    /// Held notes for the mono Granular/Wavetable textures (#339 Tier 4) — the
    /// newest still-held note sets the grain-pitch centre / table playback rate.
    /// A last-press-wins stack, so releasing the top falls back to the next-held
    /// note; recomputed to `synth_note_hz` each block (pitch-bend + A4 live).
    /// Audio-thread only.
    synth_held: keymap::HeldKeys,
    synth_note_hz: f32,
}

impl Default for OrganicMath {
    fn default() -> Self {
        Self {
            params: Arc::new(OrganicMathParams::default()),
            visual_writer: None,
            cc_override: [None; clip::N],
            last_norm: [f32::NAN; clip::N],
            release: Arc::new(AtomicBool::new(false)),
            hdr_gen: Arc::new(AtomicU32::new(0)),
            material_gen: Arc::new(AtomicU32::new(0)),
            beat_pos: Arc::new(AtomicU32::new((-1.0f32).to_bits())),
            hdr_responder: Arc::new(AtomicBool::new(false)),
            liq_reset_gen: Arc::new(AtomicU32::new(0)),
            story_next_gen: Arc::new(AtomicU32::new(0)),
            nn_gen: Arc::new(AtomicU32::new(0)),
            creature_gen: Arc::new(AtomicU32::new(0)),
            agent_on: Arc::new(AtomicBool::new(false)),
            chat_gen: Arc::new(AtomicU32::new(0)),
            plan_gen: Arc::new(AtomicU32::new(0)),
            release_gen: Arc::new(AtomicU32::new(0)),
            name_gen: Arc::new(AtomicU32::new(0)),
            model_gen: Arc::new(AtomicU32::new(0)),
            model_readout: Arc::new(std::sync::Mutex::new(String::new())),
            mind_topo: Arc::new(AtomicU32::new(0)),
            // #367 Tier 2b dial defaults: temp 0.8, ctx 2048, uncapped rate, full-attn off.
            mind_prompt_gen: Arc::new(AtomicU32::new(0)),
            mind_temp: Arc::new(AtomicU32::new(0.8f32.to_bits())),
            mind_ctx: Arc::new(AtomicU32::new(2048)),
            mind_rate: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            mind_fullattn: Arc::new(AtomicU32::new(0)),
            // **Zero, and it stays zero until an editor actually draws the pane** (#609).
            //
            // #554 T1 shipped this as `1` — "the viewport is native, not opt-in, so the
            // mirror starts requested" — and left the previous sentence ("off by default")
            // standing directly above it. The comment described a value the line did not
            // have, which is most of why the cost below went unnoticed for a month.
            //
            // The cost, since "native, not opt-in" made it sound free: a `1` here reaches
            // `Shared.mindview[3]` from the plugin's FIRST audio block, editor or not, and
            // the visual answers it with a second complete 640×360 scene render plus a
            // blocking `poll(Wait)` readback at ~15 Hz — forever, in every session. That is
            // invariant #4 (new capability defaults to inert) with the sign flipped.
            //
            // "Native, not opt-in" is still true and is not what changed: there is no user
            // toggle, and an open editor still asks unconditionally. What changed is that
            // *nobody home* now means nobody asking. See `frame_ring::mirror_requested`.
            #[cfg(not(feature = "mind-edition"))]
            viewport_on: Arc::new(AtomicU32::new(0)),
            mind_console: mind_console::MindConsole::shared(),
            // #423 atlas: inert until the first library scan; roofline inset on by default.
            atlas_gen: Arc::new(AtomicU32::new(0)),
            atlas_on: Arc::new(AtomicU32::new(0)),
            atlas_roofline: Arc::new(AtomicU32::new(1)),
            atlas_readout: Arc::new(std::sync::Mutex::new(String::new())),
            atlas_loaded_profile: Arc::new(std::sync::Mutex::new(None)),
            field_gen: Arc::new(AtomicU32::new(0)),
            nca_gen: Arc::new(AtomicU32::new(0)),
            field_load_pending: Arc::new(AtomicBool::new(false)),
            fieldclip_gen: Arc::new(AtomicU32::new(0)),
            overlay_gen: Arc::new(AtomicU32::new(0)),
            apply_gen: Arc::new(AtomicU32::new(0)),
            analyzer: None,
            loudness: None,
            cal_spectrum: None,
            audio_viz: Arc::new(audio::AudioViz::new()),
            scope: Arc::new(audio::ScopeRing::new()),
            scope_win: Vec::with_capacity(1 << 14),
            sample_rate: 48_000.0,
            audio_ring: None,
            // Resolve the saved key→preset map up front so a played note works
            // without the editor ever being opened.
            keymap: Arc::new(ArcSwap::from_pointee(keymap::KeyMap::build(
                &keymap::KeyMapping::load(),
                &preset::load(),
            ))),
            held: keymap::HeldKeys::default(),
            active_note: Arc::new(AtomicU8::new(NO_NOTE)),
            perf_mailbox: Arc::new(controller::Mailbox::new()),
            synth: synth::SynthEngine::default(),
            synth_bend: 0.0,
            synth_beat: 0.0,
            synth_beat_floor: 0,
            synth_prev_level: 0.0,
            active_bpm: Arc::new(AtomicU32::new(120.0f32.to_bits())),
            tempo_src_active: Arc::new(AtomicU32::new(0)),
            last_host_bpm: 0.0,
            last_pos_beats: f64::NAN,
            last_apply_gen: 0,
            synth_held: keymap::HeldKeys::default(),
            synth_note_hz: 0.0,
        }
    }
}

/// Build the per-block Duo-Field synth config from the live params (#339 Tier 1).
/// `bend_semi` is the current pitch-bend already scaled to the range dial.
///
/// The generative bed **follows the active field generator** so the field you see
/// is the field you hear: on **Acoustic** it sonifies that generator's own source
/// / k / separation / near / clamp; on **Maxwell** likewise (axial E). Pitch comes
/// from the field's own wavenumber `k` × `sn_tuning` (the honest ω = c·k mapping),
/// so moving the generator's `k` slider sweeps pitch. On any non-field generator
/// the manual `sn_*` bed controls are used instead.
fn build_synth_config(
    p: &OrganicMathParams,
    bend_semi: f32,
    rms: f32,
    beat_pos: f64,
    pump_env: f32,
    mallet: f32,
    note_hz: f32,
) -> synth::SynthConfig {
    use glam::Vec3;
    let tuning = p.sn_tuning.value();
    // 8 = Maxwell field, 23 = Acoustic field (see GeneratorMode).
    let gen = p.generator.value().to_u32();
    // #339 item 1 — the field responds to the music, seen AND heard. The
    // acoustic/Maxwell audio-drive toggle (#248, `ad_drive`) is reused: with it on,
    // the broadband RMS swells the bed's amplitude (`audio_drive_amp`) and breathes
    // the cavity's mode numbers (the #336 `cavity_audio_breathe` mapping), so the
    // tone gets louder + the Chladni pitch/timbre densifies with the track — the
    // audio twin of the visual's field breathe. Off (default) → no change.
    let audio_on = p.ad_drive.value() && (gen == 23 || gen == 8);
    let drive = if audio_on {
        crate::math::audio_drive_amp(p.ad_floor.value(), p.ad_amount.value(), rms)
    } else {
        1.0
    };
    let breathe = if audio_on { rms.max(0.0) } else { 0.0 };
    // Acoustic Tier 4 cavity model (ac2_model = 1 = Cavity): sonify the standing
    // wave instead of the radiating multipole. Pitch = the cavity eigenmode
    // wavenumber |k| = π·√Σ(nᵢ/Lᵢ)² × the tuning constant, so the mode numbers +
    // box size (what shapes the Chladni figure on screen) set the pitch you hear.
    let cavity = gen == 23 && p.ac2_model.value().to_u32() == 1;
    // The cavity modes move two ways, exactly like the visual: the **beat mode-walk**
    // (`cavity_morph_modes_tween` on the plugin's own beat clock — generative, no
    // audio) reorganises the Chladni set on the beat when `ac2_morph > 0`, and the
    // **audio breathe** lifts each axis by the loudness. So the sung pitch/timbre
    // move in lock-step with the nodal pattern on screen, with or without a track.
    let base_modes = glam::Vec3::new(
        p.ac2_nx.value() as f32,
        p.ac2_ny.value() as f32,
        p.ac2_nz.value() as f32,
    );
    let walked = crate::math::cavity_morph_modes_tween(
        base_modes,
        beat_pos,
        p.ac2_morph.value(),
        p.ac2_tween.value(),
    );
    let cav_modes = glam::Vec3::new(
        (walked.x + p.ac2_audio_x.value() * breathe).max(0.0),
        (walked.y + p.ac2_audio_y.value() * breathe).max(0.0),
        (walked.z + p.ac2_audio_z.value() * breathe).max(0.0),
    );
    let cav_dims = glam::Vec3::splat(p.ac2_cav_scale.value().max(1.0e-3));
    let cav_k = std::f32::consts::PI
        * ((cav_modes.x / cav_dims.x).powi(2)
            + (cav_modes.y / cav_dims.y).powi(2)
            + (cav_modes.z / cav_dims.z).powi(2))
        .sqrt();
    let (maxwell, source_kind, gen_freq, separation, near, r_min, src_count) = match gen {
        23 if cavity => (
            false,
            0u32,
            (cav_k * tuning).max(1.0),
            p.ac_separation.value(),
            p.ac_near.value(),
            p.ac_rmin.value(),
            2u32,
        ),
        23 => (
            false,
            p.ac_source.value().to_u32(),
            (p.ac_k.value() * tuning).max(1.0),
            p.ac_separation.value(),
            p.ac_near.value(),
            p.ac_rmin.value(),
            2u32,
        ),
        8 => (
            true,
            0u32,
            (p.mx_k.value() * tuning).max(1.0),
            p.mx_separation.value(),
            p.mx_near.value(),
            p.mx_rmin.value(),
            p.mx_sources.value().max(1) as u32,
        ),
        // Non-field generator: there's no radiating field to sonify, so the
        // generative bed is silent (gated below). Instrument voices still play.
        _ => (false, 0u32, 110.0, 0.6, 0.0, 0.15, 2u32),
    };
    // The generative bed only sonifies a FIELD generator (Acoustic / Maxwell).
    let field_gen = gen == 23 || gen == 8;
    synth::SynthConfig {
        on: p.sn_on.value(),
        play_mode: p.sn_play_mode.value().to_u32(),
        level: p.sn_gain.value() * p.sn_mix.value(),
        source_kind,
        maxwell,
        cavity,
        cav_modes,
        cav_dims,
        src_count,
        gen_freq,
        // #339 item 1: loudness swell (audio-drive) × the generative beat-pump
        // spike (a "speaker pushing air" on each beat; acoustic `ac_beat_pump`,
        // driven by the plugin's own beat clock — pump_env is 0 unless Pulse is on).
        gen_amp: if field_gen {
            p.sn_gen_amp.value()
                * drive
                * (1.0 + (if gen == 23 { p.ac_beat_pump.value() } else { 0.0 }) * pump_env)
        } else {
            0.0 // silent bed off a field generator
        },
        separation,
        near,
        r_min,
        probe_l: Vec3::new(p.sn_probe_lx.value(), p.sn_probe_ly.value(), p.sn_probe_lz.value()),
        probe_r: Vec3::new(p.sn_probe_rx.value(), p.sn_probe_ry.value(), p.sn_probe_rz.value()),
        a4: p.sn_a4.value(),
        bend_semi,
        place_spread: p.sn_place_spread.value(),
        attack: p.sn_attack.value(),
        decay: p.sn_decay.value(),
        sustain: p.sn_sustain.value(),
        release: p.sn_release.value(),
        glide: p.sn_glide.value(),
        vis_pivot: p.sn_vis_pivot.value(),
        vis_anchor: p.sn_vis_anchor.value(),
        vis_slope: p.sn_vis_slope.value(),
        vis_k_anchor: p.sn_vis_k_anchor.value(),
        vis_k_slope: p.sn_vis_k_slope.value(),
        vis_quantize: p.sn_vis_quantize.value().to_u32(),
        // #339 Tier 2 oscillator lattice. `field_k` = the generator's RAW wavenumber
        // (spatial nodal structure on the shell), distinct from the audio pitch.
        mode: p.sn_mode.value().to_u32(),
        bank_size: p.sn_bank.value().max(1) as u32,
        tuning_layout: p.sn_tuning_layout.value().to_u32(),
        tune_spread: p.sn_tune_spread.value(),
        tune_stretch: p.sn_tune_stretch.value(),
        shell_r: p.sn_shell_r.value(),
        shell_rate: p.sn_shell_rate.value(),
        field_k: match gen {
            23 => p.ac_k.value(),
            8 => p.mx_k.value(),
            _ => 1.5,
        },
        // #339 Tier 3 modal synthesis.
        mallet,
        // Modal is struck, not a field bed — its amplitude is the bed-amp dial and
        // is NOT field-gated (so it sounds on any generator / in Instrument mode).
        modal_amp: p.sn_gen_amp.value(),
        t60: p.sn_t60.value(),
        bright: p.sn_bright.value(),
        // #339 Tier 4 granular + wavetable.
        note_hz,
        grain_size: p.sn_grain_size.value(),
        grain_density: p.sn_grain_density.value(),
    }
}

impl Drop for OrganicMath {
    fn drop(&mut self) {
        // Stop the #354 HDR responder thread (it exits within its ~50 ms poll).
        self.hdr_responder.store(false, Ordering::Relaxed);
    }
}

impl Plugin for OrganicMath {
    // #483 Tier 1 — the product name follows the build-time edition. A default
    // (feature-off) build resolves to exactly `"Organon"`, so the host-facing plugin
    // identity is untouched; `--features mind-edition` names the standalone
    // "Organon Mind". (The VST3 class ID / CLAP ID below are NOT edition-dependent
    // and must never change — Organon Mind is standalone-only and needs no plugin ID.)
    const NAME: &'static str = crate::edition::EDITION.product_name();
    // #673 — the project's own identity, not the studio account it was first built under.
    // These three are free to change: a host groups by VENDOR in its browser and shows
    // URL/EMAIL in the plugin's info panel, but a saved session restores a plugin by its
    // **class ID**, which is the line that must never move (`CLAP_ID` /
    // `VST3_CLASS_ID` below, `com.amplifyluxury.organic-math` / `OrganicMathViz01`).
    // "amplifyluxury" therefore stays visible in the CLAP ID forever, on purpose.
    const VENDOR: &'static str = "Organon";
    const URL: &'static str = "https://organon.art";
    const EMAIL: &'static str = "hello@organon.art";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Stereo pass-through so it can sit on a track and receive transport + MIDI.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Open the shared-memory channel the visual window reads from.
        self.visual_writer = ipc::Writer::create().ok();
        // Build the audio-reactive analyzer for this sample rate.
        self.sample_rate = buffer_config.sample_rate;
        // #430: (re)create the audio-sample ring at the host rate so the visual's
        // recorder can capture "whatever audio was flowing". Best-effort — a failure
        // just means recordings are video-only.
        self.audio_ring = audio_ring::AudioRingWriter::create(buffer_config.sample_rate as u32).ok();
        self.analyzer = Some(audio::Analyzer::new(buffer_config.sample_rate));
        self.loudness = Some(audio::LoudnessMeter::new(buffer_config.sample_rate));
        self.cal_spectrum = Some(audio::CalibratedSpectrum::new(buffer_config.sample_rate));
        // #339 Duo-Field synthesis: size the field-probe DSP to this sample rate.
        self.synth.set_sample_rate(buffer_config.sample_rate);
        self.synth.release_all();

        // #354: spawn the HDR responder once. It watches `active_note` (published
        // by `process()`) and makes a MIDI-held Scene preset's captured `.hdr`
        // environment follow — the sidecar write + `hdr_gen` bump run HERE, off
        // the audio thread, and revert to the pre-hold `.hdr` on release so the
        // whole environment (params AND IBL image) tracks the held preset. A GUI
        // preset recall does the same via `apply_recall`. Polls at ~50 ms (edge
        // on `active_note`), only reloads when the target path actually differs,
        // so it can't thrash the IBL precompute while a note is simply held.
        if !self.hdr_responder.swap(true, Ordering::AcqRel) {
            let alive = self.hdr_responder.clone();
            let active_note = self.active_note.clone();
            let keymap = self.keymap.clone();
            let hdr_gen = self.hdr_gen.clone();
            std::thread::spawn(move || {
                let mut last_note = NO_NOTE;
                let mut base_hdr: Option<String> = None; // sidecar before we swapped
                let mut swapped = false;
                while alive.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let note = active_note.load(Ordering::Relaxed);
                    if note == last_note {
                        continue;
                    }
                    last_note = note;
                    if note != NO_NOTE {
                        // A mapped Scene note became active — follow its `.hdr`.
                        let target = keymap
                            .load()
                            .get(note)
                            .map(|pv| pv.hdr_path.clone())
                            .unwrap_or_default();
                        if !target.is_empty() {
                            let cur = std::fs::read_to_string(ipc::hdr_sidecar_path())
                                .unwrap_or_default();
                            if target != cur {
                                if !swapped {
                                    base_hdr = Some(cur); // remember to restore
                                }
                                if std::fs::write(ipc::hdr_sidecar_path(), target.as_bytes())
                                    .is_ok()
                                {
                                    hdr_gen.fetch_add(1, Ordering::Relaxed);
                                    swapped = true;
                                }
                            }
                        }
                    } else if swapped {
                        // All mapped notes released — restore the pre-hold `.hdr`.
                        if let Some(prev) = base_hdr.take() {
                            let _ = std::fs::write(ipc::hdr_sidecar_path(), prev.as_bytes());
                            hdr_gen.fetch_add(1, Ordering::Relaxed);
                        }
                        swapped = false;
                    }
                }
            });
        }
        true
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        // #593 Tier 0 — the route-C probe. Deliberately wired *here* rather than into its
        // own binary: this is the real host path Tier 2 has to work on (`mind_main.rs` →
        // `nih_export_standalone` → `Editor::spawn` with an `AppKitNsView`), and it is the
        // same seam Tier 2 grows into. Two gates — the `mind-edition` feature and an env
        // var — so the shipping plugin neither changes size nor behaviour.
        //
        //     ORGANON_EDITOR_PROBE=1 ./organon-mind --backend dummy
        //
        // (`--backend dummy` is required: real CoreAudio hard-aborts the standalone, #579.)
        #[cfg(feature = "mind-edition")]
        if editor_probe::probe_requested() {
            nih_log!("#593 Tier 0: {}=1 — opening the editor probe instead of the editor",
                editor_probe::PROBE_ENV);
            return Some(Box::new(editor_probe::ProbeEditor::default()));
        }

        let params = self.params.clone();
        let release = self.release.clone();
        let hdr_gen = self.hdr_gen.clone();
        // #554 T1 — the editor's half of the viewport toggle; `process()` stamps it into
        // `Shared.mindview[3]` so the visual knows whether to publish frames at all.
        // #593 Tier 4 — full Organon only; Mind has no mirror to ask for.
        #[cfg(not(feature = "mind-edition"))]
        let viewport_on = self.viewport_on.clone();
        let material_gen = self.material_gen.clone();
        let beat_pos = self.beat_pos.clone();
        let overlay_gen = self.overlay_gen.clone();
        let liq_reset_gen = self.liq_reset_gen.clone();
        let story_next_gen = self.story_next_gen.clone();
        let nn_gen = self.nn_gen.clone();
        let creature_gen = self.creature_gen.clone();
        let agent_on = self.agent_on.clone();
        let chat_gen = self.chat_gen.clone();
        let plan_gen = self.plan_gen.clone();
        let release_gen = self.release_gen.clone();
        let name_gen = self.name_gen.clone();
        let model_gen = self.model_gen.clone();
        let model_readout = self.model_readout.clone();
        let mind_topo = self.mind_topo.clone();
        let mind_prompt_gen = self.mind_prompt_gen.clone();
        let mind_temp = self.mind_temp.clone();
        let mind_ctx = self.mind_ctx.clone();
        let mind_rate = self.mind_rate.clone();
        let mind_fullattn = self.mind_fullattn.clone();
        let mind_console = self.mind_console.clone();
        let atlas_gen = self.atlas_gen.clone();
        let atlas_on = self.atlas_on.clone();
        let atlas_roofline = self.atlas_roofline.clone();
        let atlas_readout = self.atlas_readout.clone();
        let atlas_loaded_profile = self.atlas_loaded_profile.clone();
        let field_gen = self.field_gen.clone();
        let nca_gen = self.nca_gen.clone();
        let field_load_pending = self.field_load_pending.clone();
        let fieldclip_gen = self.fieldclip_gen.clone();
        let apply_gen = self.apply_gen.clone();
        let keymap = self.keymap.clone();
        let active_note = self.active_note.clone();
        let audio_viz = self.audio_viz.clone();
        let scope = self.scope.clone();
        let active_bpm = self.active_bpm.clone();
        let tempo_src_active = self.tempo_src_active.clone();
        let perf_mailbox = self.perf_mailbox.clone();
        // #520 Tier 2 — `ResizableWindow` writes the dragged size back through
        // `EguiState::set_requested_size`, so the update closure needs its own
        // handle to the same state `create_egui_editor` is given below.
        let editor_state = self.params.editor_state.clone();
        // #593 Tier 4 — does the host chosen below draw the world *behind* this interface?
        // Computed once, here, so the `EditorCtx` field and the branch that returns the wgpu
        // editor cannot disagree about which host is being built.
        #[cfg(feature = "mind-edition")]
        let scene_behind = wgpu_editor::wgpu_editor_enabled();
        #[cfg(not(feature = "mind-edition"))]
        let scene_behind = false;
        // #593 Tier 1 — everything the editor body used to capture crosses into
        // `editor_ui` in one struct, so a second host can call the identical code.
        let cx = EditorCtx {
            params,
            release,
            hdr_gen,
            #[cfg(not(feature = "mind-edition"))]
            viewport_on,
            material_gen,
            beat_pos,
            overlay_gen,
            liq_reset_gen,
            story_next_gen,
            nn_gen,
            creature_gen,
            agent_on,
            chat_gen,
            plan_gen,
            release_gen,
            name_gen,
            model_gen,
            model_readout,
            mind_topo,
            mind_prompt_gen,
            mind_temp,
            mind_ctx,
            mind_rate,
            mind_fullattn,
            mind_console,
            atlas_gen,
            atlas_on,
            atlas_roofline,
            atlas_readout,
            atlas_loaded_profile,
            field_gen,
            nca_gen,
            field_load_pending,
            fieldclip_gen,
            apply_gen,
            keymap,
            active_note,
            audio_viz,
            scope,
            active_bpm,
            tempo_src_active,
            perf_mailbox,
            editor_state,
            scene_behind,
        };
        // #593 — Organon Mind's own wgpu editor: the scene and this same interface on one
        // device, in this same window. Placed *after* `cx` so both hosts are handed the
        // identical `EditorCtx` (whichever wins simply takes ownership of it), and gated on the
        // `mind-edition` feature so the shipping plugin neither changes size nor behaviour.
        //
        // **This is Mind's editor now.** For five tiers it also needed `ORGANON_EDITOR_WGPU=1`
        // — house invariant #6, new capability defaults to inert — and the Mac pass that gate
        // was waiting on happened 2026-08-03. `ORGANON_EDITOR_WGPU=0` is the way back, and it
        // is a bring-up fallback rather than a mode: the `nih_plug_egui` editor below has no
        // viewport in a mind-edition build, because Tier 4 gated the #554 mirror pane out of
        // Mind's path. Leaving the old default in place meant shipping an instrument that
        // could not show you the model.
        //
        //     ./organon-mind --backend dummy
        //
        // (`--backend dummy` is required: real CoreAudio hard-aborts the standalone, #579.)
        #[cfg(feature = "mind-edition")]
        if scene_behind {
            nih_log!("#593: opening the wgpu editor (scene + UI on one device)");
            return Some(Box::new(wgpu_editor::WgpuEditor::new(
                cx,
                self.params.editor_state.clone(),
            )));
        }
        // Reached in a mind-edition build only with `ORGANON_EDITOR_WGPU=0`, and worth saying
        // out loud: the editor below has **no viewport** here, because Tier 4 gated the #554
        // mirror pane out of Mind's path. "No viewport" is the single most misread state in
        // this product — a Mind window with no specimen in it looks exactly like a broken one,
        // and that has cost real time more than once. Do not let it be silent.
        #[cfg(feature = "mind-edition")]
        nih_warn!(
            "{}=0 — falling back to the egui editor, which has NO viewport in this build. \
             Unset it to get the scene back.",
            wgpu_editor::WGPU_EDITOR_ENV
        );
        create_egui_editor(
            self.params.editor_state.clone(),
            preset::PresetUi::default(),
            |_, state: &mut preset::PresetUi| {
                if !state.loaded {
                    state.presets = preset::load();
                    for tab in preset::EditorTab::ALL {
                        state.tab_presets[tab.index()] = preset::load_tab(tab);
                    }
                    state.loaded = true;
                }
                if !state.keymap_loaded {
                    state.mapping = keymap::KeyMapping::load();
                    state.keymap_octave = 3; // open on the octave around middle C
                    state.keymap_loaded = true;
                }
            },
            move |ctx, setter, state| editor_ui(&cx, ctx, setter, state),
        )
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Manual "release" from the editor clears all clip overrides at once.
        if self.release.swap(false, Ordering::Relaxed) {
            self.cc_override = [None; clip::N];
        }

        // Audio-reactive analysis: extract band envelopes from the track's input
        // (we're a stereo pass-through, so `buffer` is the incoming audio). The
        // analyzer is allocation-free; this stays audio-thread safe. Param values
        // are read into locals first so the `&mut self.analyzer` borrow is clean.
        // #346 Field Chamber: the right-wall spectrum reads the calibrated RTA
        // (`audiospectrum` + `audiometer[11..14]` band header) which is produced ONLY by
        // the analysis below. So run the analysis when the spectrum panel is effectively
        // on — the raw params OR a held key-map preset (matching the scope-publish gate) —
        // else the wall shows no bars while the scope runs live (Bugbot).
        let spectrum_wall_on = (self.params.panels_on.value() && self.params.panel_right.value())
            || {
                let km = self.keymap.load();
                match self.held.top_mapped(&km).and_then(|n| km.get(n)) {
                    // km.get now returns the preset's PresetValues (#354).
                    Some(pv) => pv.panels_on && pv.panel_right,
                    None => false,
                }
            };
        let viz_active = self.params.audio_react.value() || spectrum_wall_on;
        // #333: calibrated metering block written into the snapshot below. dB fields
        // floor at −120; [3] LRA and [5] correlation are linear; [10] mirrors the HUD
        // toggle so the visual can draw the numeric readout.
        let meter_hud = self.params.meter_hud.value();
        let mut audiometer = [-120.0f32; 16];
        audiometer[3] = 0.0;
        audiometer[5] = 0.0;
        audiometer[10] = if meter_hud { 1.0 } else { 0.0 };
        for v in audiometer[11..].iter_mut() {
            *v = 0.0;
        }
        // #333 Tier 2: the calibrated RTA band levels (dBFS) for the snapshot.
        let mut audiospectrum = [-120.0f32; 128];
        // #333: whether the calibrated meters got real values this block. When Audio
        // Reactive is off we publish silent below, so the editor never reads a
        // stale/zero (= full-scale) meter. Publishing the real meters happens inside
        // the analysis arm directly from the borrowed slices — no audio-thread alloc.
        let mut meters_published = false;

        // #333 oscilloscope: always capture the raw stereo waveform (cheap — two
        // relaxed stores/sample), independent of the FFT analysis toggle, so the
        // scope works whether or not Audio Reactive is on.
        self.scope.set_sample_rate(self.sample_rate);
        for mut frame in buffer.iter_samples() {
            let mut it = frame.iter_mut();
            let l = it.next().map(|s| *s).unwrap_or(0.0);
            let r = it.next().map(|s| *s).unwrap_or(l);
            self.scope.push(l, r);
        }
        let (bands, level, peak, spectrum, centroid, balance, cam_bpm, cam_bpm_conf) = if viz_active {
            let gain = self.params.audio_gain.value();
            let attack = self.params.audio_attack.value();
            let release = self.params.audio_release.value();
            let res = self.params.meter_res.value().denom();
            let weight = self.params.meter_weight.value().to_u32();
            let avg = self.params.meter_averaging.value().to_u32();
            let sr = self.sample_rate;
            let n = buffer.samples();
            // Disjoint field borrows so the analyzer, loudness meter and calibrated
            // spectrum can all be driven from the one frame loop.
            match (self.analyzer.as_mut(), self.loudness.as_mut(), self.cal_spectrum.as_mut()) {
                (Some(an), Some(lm), Some(cs)) => {
                    an.set_sample_rate(sr);
                    an.set_times(attack, release);
                    lm.set_sample_rate(sr);
                    // Sum each frame's channels to mono for the FFT ring; feed the raw
                    // L/R to the calibrated loudness/true-peak meter (#333 Tier 1);
                    // accumulate per-side energies for the stereo balance (#248 T3 —
                    // channel 0 = L, 1 = R; mono buffers land centred).
                    let mut l_sq = 0.0f32;
                    let mut r_sq = 0.0f32;
                    for mut frame in buffer.iter_samples() {
                        let mut acc = 0.0f32;
                        let mut cnt = 0u32;
                        let mut l0 = 0.0f32;
                        let mut r0 = 0.0f32;
                        for s in frame.iter_mut() {
                            if cnt == 0 {
                                l_sq += *s * *s;
                                l0 = *s;
                            } else if cnt == 1 {
                                r_sq += *s * *s;
                                r0 = *s;
                            }
                            acc += *s;
                            cnt += 1;
                        }
                        if cnt == 1 {
                            r_sq = l_sq; // mono: both sides carry the same energy
                            r0 = l0;
                        }
                        an.push_sample(if cnt > 0 { acc / cnt as f32 } else { 0.0 });
                        lm.push(l0, r0);
                    }
                    let dt = (n as f32 / sr).max(1.0e-4);
                    let bands = an.analyze(gain, dt);
                    let bal = an.update_balance(l_sq, r_sq, dt);
                    // #333 Tier 2: integrate the analyzer's raw FFT power into the
                    // calibrated fractional-octave RTA.
                    cs.configure(sr, res, weight, avg);
                    cs.update(an.raw_bins(), audio::FFT_SIZE, an.window_energy(), dt);
                    // #333 Tier 1: gather the calibrated meters → snapshot + editor.
                    let lr = lm.levels_lrms();
                    let m = [
                        lm.momentary(),
                        lm.short_term(),
                        lm.integrated(),
                        lm.lra(),
                        lm.true_peak_db(),
                        lm.correlation(),
                        lr[0],
                        lr[1],
                        lr[2],
                        lr[3],
                    ];
                    for i in 0..10 {
                        audiometer[i] = if m[i].is_finite() { m[i] } else { -120.0 };
                    }
                    // Calibrated RTA → snapshot (audiospectrum) + editor (AudioViz).
                    let levels = cs.levels_db();
                    let nb = levels.len().min(128);
                    for i in 0..nb {
                        audiospectrum[i] = if levels[i].is_finite() { levels[i] } else { -120.0 };
                    }
                    audiometer[11] = nb as f32;
                    audiometer[12] = res as f32;
                    audiometer[13] = cs.center_hz(0);
                    // Publish directly from the borrowed slices (no alloc on the audio
                    // thread — `&m` is a stack array, `levels` is a view into `cs`).
                    self.audio_viz.publish_meters(&m, levels, res, cs.center_hz(0));
                    meters_published = true;
                    (
                        bands,
                        an.input_level(),
                        an.input_peak(),
                        *an.display_spectrum(),
                        an.spectral_centroid(),
                        bal,
                        an.tempo_bpm(),
                        an.tempo_confidence(),
                    )
                }
                _ => ([0.0; audio::NUM_BANDS], 0.0, 0.0, [0.0; audio::DISP_BINS], 0.0, 0.0, 0.0, 0.0),
            }
        } else {
            ([0.0; audio::NUM_BANDS], 0.0, 0.0, [0.0; audio::DISP_BINS], 0.0, 0.0, 0.0, 0.0)
        };
        // Publish the live analysis to the editor's Audio panel (lock-free). The
        // calibrated meters/RTA publish every block — silent when Audio Reactive is
        // off — so the editor's readouts + true-peak alarm never latch on stale or
        // zero-initialised (= full-scale 0 dBFS) values.
        self.audio_viz
            .publish(viz_active, level, peak, &bands, &spectrum);
        // When Audio Reactive is off (no real meters this block), publish silence so
        // the editor's readouts + true-peak alarm don't latch on stale values.
        if !meters_published {
            self.audio_viz.publish_meters(&audio::SILENT_METERS, &[], 3, 0.0);
        }

        // Incoming MIDI: parameter CCs (from a played/dropped clip) override the
        // look; note on/off drive the Key Map (a held mapped note activates its
        // preset). A note-on with zero velocity is a note-off by convention.
        //
        // #339 Duo-Field synthesis: when the synth is on in Instrument/Duet mode,
        // notes drive the VOICE engine (each note a radiating source) and the Key
        // Map is bypassed; otherwise the current Key-Map behaviour is untouched.
        // The Duo-Field synth only runs on the field generators it sonifies
        // (Maxwell = 8, Acoustic = 23); on any other generator it's inert (a
        // byte-identical passthrough), matching the editor card being hidden there.
        let synth_gen = self.params.generator.value().to_u32();
        let synth_on =
            self.params.sn_on.value() && (synth_gen == 8 || synth_gen == 23);
        let play_mode = self.params.sn_play_mode.value().to_u32();
        let synth_notes = synth_on && (play_mode == 1 || play_mode == 2);
        let bend_range = self.params.sn_bend_range.value();
        let synth_a4 = self.params.sn_a4.value();
        // #339 Tier 3: in Modal mode a played note is a MALLET striking the cavity
        // (retuning + ringing the eigenmodes), not a radiating voice.
        let sn_mode = self.params.sn_mode.value().to_u32();
        let modal_mode = synth_on && sn_mode == 2;
        // #339 Tier 4: Granular (3) + Wavetable (4) are mono textures — a played
        // note sets the pitch centre (grain pitch / table playback rate); with no
        // note held the mode falls back to its own base frequency.
        let tone_mode = synth_on && (sn_mode == 3 || sn_mode == 4);
        // #356: when the Performance Controller is on, forward raw pad/button MIDI
        // to the editor's GUI thread via the wait-free mailbox. Kept additive —
        // the existing Key Map / synth / clip-CC handling below is unchanged, so
        // the surface layer is inert when `perf_enable` is off.
        let perf_on = self.params.perf_enable.value();
        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::MidiCC { channel, cc, value, .. } => {
                    // Always mirror incoming MIDI into the controller mailbox so the
                    // diagnostic readout reflects true reception regardless of the
                    // enable state (a blank readout then means MIDI isn't arriving,
                    // not "controller off"). Only ACT on it — and only then keep it
                    // off the MIDI-clip CC map (some Launchpad CCs overlap CC 16+) —
                    // when the controller is enabled.
                    self.perf_mailbox.push(controller::RawMidi::cc(
                        cc,
                        (value * 127.0).round() as u8,
                        channel,
                    ));
                    if !perf_on {
                        if let Some(i) = clip::cc_index(cc) {
                            self.cc_override[i] = Some(value);
                        }
                    }
                }
                NoteEvent::MidiPitchBend { value, .. } => {
                    // value ∈ [0,1], 0.5 = centre → ± range semitones.
                    self.synth_bend = (value - 0.5) * 2.0 * bend_range;
                }
                NoteEvent::NoteOn { channel, note, velocity, .. } => {
                    // Always mirror to the mailbox (diagnostic). When the controller
                    // owns the surface (enabled), notes go ONLY to it — not the
                    // momentary, Scene-recalling Key Map or the synth — so a pad
                    // press can't double-fire and revert on release.
                    self.perf_mailbox.push(controller::RawMidi::note_on(
                        note,
                        (velocity * 127.0).round() as u8,
                        channel,
                    ));
                    if perf_on {
                        // controller owns the note; handled by the editor drain.
                    } else if synth_notes {
                        if velocity > 0.0 {
                            // Track the held stack for the mono tone modes regardless
                            // of the current engine, so switching into Granular/Wave-
                            // table with a key already down sounds immediately.
                            self.synth_held.press(note);
                            if modal_mode {
                                let f = crate::math::note_freq(note as f32, self.synth_bend, synth_a4);
                                self.synth.strike_modal(f, velocity);
                            } else if !tone_mode {
                                self.synth.note_on(note, velocity, self.synth_bend, synth_a4);
                            }
                            // tone_mode: pitch is recomputed per-block from synth_held.
                        } else {
                            self.synth_held.release(note);
                            if !tone_mode {
                                self.synth.note_off(note);
                            }
                        }
                    } else if velocity > 0.0 {
                        self.held.press(note);
                    } else {
                        self.held.release(note);
                    }
                }
                NoteEvent::NoteOff { channel, note, .. } => {
                    self.perf_mailbox
                        .push(controller::RawMidi::note_off(note, channel));
                    if perf_on {
                        // controller owns the note; handled by the editor drain.
                    } else if synth_notes {
                        self.synth_held.release(note);
                        if !tone_mode {
                            self.synth.note_off(note);
                        }
                    } else {
                        self.held.release(note);
                    }
                }
                _ => {}
            }
        }

        // #339 Tier 4 — mono Granular/Wavetable pitch: recompute each block from the
        // newest still-held note so pitch-bend and concert-A changes apply live, and
        // releasing the top key falls back to the next-held note. No key held (or not
        // a tone engine) → 0, i.e. the mode falls back to its own base frequency.
        self.synth_note_hz = if tone_mode {
            match self.synth_held.top() {
                Some(n) => crate::math::note_freq(n as f32, self.synth_bend, synth_a4),
                None => 0.0,
            }
        } else {
            0.0
        };

        // #339 item 1 — the synth's own beat clock, so the beat pump + cavity
        // mode-walk + modal mallet move the SOUND generatively. Resolves the active
        // BPM the SAME way the visual does (`math::resolve_bpm` honoring the Sync/
        // Tempo card's clock source — Host / Audio / Manual), so the MANUAL dial
        // actually drives the synth (was: always the host tempo). Host source +
        // sync + playing snaps to the DAW beat grid; Audio/Manual free-run.
        let tempo_source = self.params.tempo_source.value().to_u32();
        // A discontinuous jump in the beat position (host-lock engaging, a seek, or a
        // loop wrap) must NOT be read as a musical beat crossing (which would fire a
        // spurious modal strike). Compare the new position to where a plain free-run
        // would have landed.
        let mut beat_discontinuous = false;
        let active_bpm = {
            let t = context.transport();
            // Hold the last valid host tempo so a stopped/omitting host doesn't drop
            // Host mode back to the manual dial.
            if let Some(bpm) = t.tempo.filter(|&x| x > 0.0) {
                self.last_host_bpm = bpm;
            }
            let host_has = self.last_host_bpm > 0.0;
            let bpm = crate::math::resolve_bpm(
                tempo_source,
                self.params.tempo.value() as f64,
                self.last_host_bpm,
                host_has,
                cam_bpm as f64,
            );
            let dt_beats = buffer.samples() as f64 / self.sample_rate.max(1.0) as f64 * bpm / 60.0;
            // Snap to the host beat grid only when Host source + the PLL toggle +
            // playing + `pos_beats` is actually ADVANCING (a host that withholds it
            // from the effect would freeze the beat otherwise). Else free-run at the
            // resolved BPM — which still follows the host tempo.
            let pos = t.pos_beats();
            let advancing = pos.map_or(false, |p| {
                let d = p - self.last_pos_beats;
                self.last_pos_beats.is_finite() && d.abs() > 1.0e-9 && d > -512.0
            });
            if let Some(p) = pos {
                self.last_pos_beats = p;
            }
            let host_lock =
                tempo_source == 0 && self.params.tempo_sync.value() && t.playing && host_has && advancing;
            let free_run = self.synth_beat + dt_beats;
            self.synth_beat = match (host_lock, pos) {
                (true, Some(p)) => p,
                _ => free_run,
            };
            // A jump of more than a quarter-beat off the free-run target is a
            // re-sync, not a beat boundary.
            beat_discontinuous = (self.synth_beat - free_run).abs() > 0.25;
            bpm
        };
        // Publish the active BPM + source for the editor's perf-footer readout
        // (lock-free; the editor shows exactly what the synth beat clock uses).
        self.active_bpm.store((active_bpm as f32).to_bits(), Ordering::Relaxed);
        self.tempo_src_active.store(tempo_source, Ordering::Relaxed);
        let pump_env = if self.params.pulse.value() {
            (-(self.synth_beat.rem_euclid(1.0) as f32) * 6.0).exp()
        } else {
            0.0
        };

        // #339 Tier 3 mallet: the generative strike that hits the modal cavity — a
        // BEAT crossing (Pulse on) or an input TRANSIENT (a rising RMS edge, so the
        // track beats the cavity). Only generative/duet play modes auto-strike;
        // Instrument mode strikes from note-on. Consumed only in Modal mode.
        let beat_floor = self.synth_beat.floor() as i64;
        // A real beat crossing = the integer beat advanced by a normal step (not a
        // re-sync jump), so a host-lock snap / seek / loop wrap doesn't false-trigger.
        let beat_crossed = !beat_discontinuous && beat_floor != self.synth_beat_floor;
        self.synth_beat_floor = beat_floor;
        // Raw input RMS for the transient detector — computed from the buffer itself
        // (before the synth writes it), so the "track beats the cavity" path works
        // WITHOUT the visual's audio-analyzer (`audio_react`) being on. Modal only.
        let in_rms = if synth_on && modal_mode {
            let mut sq = 0.0f32;
            let mut cnt = 0u32;
            for mut frame in buffer.iter_samples() {
                for s in frame.iter_mut() {
                    sq += *s * *s;
                    cnt += 1;
                }
            }
            if cnt > 0 { (sq / cnt as f32).sqrt() } else { 0.0 }
        } else {
            0.0
        };
        let mallet = if play_mode == 1 {
            0.0
        } else {
            let beat_strike = if self.params.pulse.value() && beat_crossed { 0.9f32 } else { 0.0 };
            let trans_strike = if in_rms > self.synth_prev_level * 1.6 + 0.03 {
                (in_rms * 4.0).min(1.0)
            } else {
                0.0
            };
            beat_strike.max(trans_strike)
        };
        self.synth_prev_level = in_rms;

        // Render the synth bus over the passthrough (after the analyzer has read
        // the untouched input at the top, so the audio-reactive spine keeps
        // analyzing the TRACK, not our own output). Off → the buffer is untouched.
        if synth_on {
            let cfg = build_synth_config(&self.params, self.synth_bend, level, self.synth_beat, pump_env, mallet, self.synth_note_hz);
            self.synth.retune(cfg.bend_semi, cfg.a4);
            let chans = buffer.as_slice();
            if chans.len() >= 2 {
                let (l, rest) = chans.split_at_mut(1);
                self.synth.render(&mut *l[0], &mut *rest[0], &cfg);
            }
        }

        // #430: stream the post-synth stereo output (passthrough + synth = "whatever was
        // flowing") into the audio ring for the visual's recorder. Mirrors the scope loop
        // above; two mmap stores per sample, no allocation — audio-thread safe, and the
        // buffer itself is untouched (audio output byte-identical).
        if let Some(ar) = self.audio_ring.as_mut() {
            for mut frame in buffer.iter_samples() {
                let mut it = frame.iter_mut();
                let l = it.next().map(|s| *s).unwrap_or(0.0);
                let r = it.next().map(|s| *s).unwrap_or(l);
                ar.push_frame(l, r);
            }
        }

        // Push the live snapshot (with any CC overrides applied) to the visual
        // process. Just memory stores into the mmap — audio-thread safe.
        let apply_gen0 = self.apply_gen.load(Ordering::Acquire);
        // A completed preset recall (apply_gen advanced to an even value) damps the
        // modal ring so it doesn't bleed into the new patch — held voices survive.
        if apply_gen0 != self.last_apply_gen {
            if apply_gen0 & 1 == 0 {
                self.synth.damp_modal();
            }
            self.last_apply_gen = apply_gen0;
        }
        if let Some(w) = self.visual_writer.as_mut() {
            // Atomic preset recall: if an apply is mid-flight (odd generation),
            // don't publish a half-applied snapshot — the visual keeps the last
            // complete one. (See `apply_atomic`.)
            if apply_gen0 & 1 == 1 {
                // Still keep the host from suspending us while generating sound.
                return if synth_on { ProcessStatus::KeepAlive } else { ProcessStatus::Normal };
            }
            let base = self.params.to_shared();

            // Release a CC override as soon as its slider/automation moves, so
            // moving a slider always wins control back from a clip.
            for i in 0..clip::N {
                let cur = clip::normalized(&base, i);
                if (cur - self.last_norm[i]).abs() > 1e-4 {
                    self.cc_override[i] = None;
                }
                self.last_norm[i] = cur;
            }

            let mut snapshot = base;
            for (i, ov) in self.cc_override.iter().enumerate() {
                if let Some(norm) = ov {
                    clip::apply_normalized(&mut snapshot, i, *norm);
                }
            }

            // Key Map: a held, mapped note overlays its **Scene** (Generator /
            // Motion / Environment / Look) onto the LIVE look (#354), winning over
            // the sliders + any clip for those four tabs while leaving Settings /
            // Audio / Synth at their live values — mirroring a GUI Scene recall.
            // The overlay is done here (audio thread) from a fresh live capture, so
            // it tracks slider/host-restore changes without a Key Map rebuild. The
            // live transport/audio/*_gen fields are stamped on just below.
            let km = self.keymap.load();
            let active = self.held.top_mapped(&km);
            if let Some(note) = active {
                if let Some(pv) = km.get(note) {
                    // Live capture (no `.hdr` sidecar read → audio-thread safe),
                    // then overlay only the preset's four Scene tabs.
                    let mut merged = preset::PresetValues::capture_params_only(&self.params);
                    merged.overlay_tabs(pv, &preset::EditorTab::SCENE);
                    snapshot = merged.to_shared();
                    // `capture` doesn't cover the runtime / per-display-quality
                    // fields, so `to_shared` left them at defaults — restore the
                    // live ones from `base` (transport / audio / *_gen are
                    // re-stamped just below regardless).
                    snapshot.voices = base.voices;
                    snapshot.temporal = base.temporal;
                    snapshot.pathtrace_on = base.pathtrace_on;
                    snapshot.rt[1] = base.rt[1];
                }
            }
            self.active_note
                .store(active.unwrap_or(NO_NOTE), Ordering::Relaxed);

            snapshot.hdr_gen = self.hdr_gen.load(Ordering::Relaxed);
            snapshot.material_gen = self.material_gen.load(Ordering::Relaxed); // #472 T1 material folder load
            snapshot.overlay_gen = self.overlay_gen.load(Ordering::Relaxed);
            snapshot.nn_gen = self.nn_gen.load(Ordering::Relaxed); // #226 T3 connectome load
            snapshot.creature_gen = self.creature_gen.load(Ordering::Relaxed); // #476 T2b creature-JSON load
            snapshot.field_gen = self.field_gen.load(Ordering::Relaxed); // #381 field program load
            snapshot.fieldclip_gen = self.fieldclip_gen.load(Ordering::Relaxed); // #407 Tier A field clip load
            snapshot.nca_gen = self.nca_gen.load(Ordering::Relaxed); // #407 Tier B NCA model load
            // AI-Performer (#317 T1) runtime block: [agent_on, chat_gen, plan_gen, release_gen].
            snapshot.agent[0] = self.agent_on.load(Ordering::Relaxed) as u32 as f32;
            snapshot.agent[1] = self.chat_gen.load(Ordering::Relaxed) as f32;
            snapshot.agent[2] = self.plan_gen.load(Ordering::Relaxed) as f32;
            snapshot.agent[3] = self.release_gen.load(Ordering::Relaxed) as f32;
            // #425 intelligent preset names: name_gen bumps on save (auto-naming on).
            snapshot.agent[4] = self.name_gen.load(Ordering::Relaxed) as f32;
            // Visible-mind specimen (#367 T1) — runtime-stamped mind block: model_gen
            // counter (slot 1) drives the visual's GGUF load; mind_on (slot 0) = a model
            // has been picked; topo_mode (slot 2) = 0 (architecture skeleton; Tier-2 res.).
            let model_gen = self.model_gen.load(Ordering::Relaxed);
            snapshot.mind[0] = if model_gen > 0 { 1.0 } else { 0.0 };
            snapshot.mind[1] = model_gen as f32;
            // Mind VIEW selector → the reserved mind[2] slot (no Shared
            // size/LAYOUT_VERSION change): 0 = the #367 Tier 1 architecture specimen,
            // 1 = the #507 Tier 1 embedding galaxy. "Live" is NOT a view — the glow
            // rides the activation ring whenever frames arrive, on whichever geometry
            // is selected. Clamped, so a future value can only ever fall back to the
            // specimen rather than selecting something unknown.
            snapshot.mind[2] = self.mind_topo.load(Ordering::Relaxed).min(1) as f32;
            // #367 Tier 2b — embedded-runtime dials → the reserved mind[3..8] slots (no
            // Shared size/LAYOUT_VERSION change). The optional `organic-math-mind-runtime` bin
            // reads them: prompt_gen (edge-detected → run one completion), temperature,
            // context length, token-rate cap, full-attention toggle.
            snapshot.mind[3] = self.mind_prompt_gen.load(Ordering::Relaxed) as f32;
            snapshot.mind[4] = f32::from_bits(self.mind_temp.load(Ordering::Relaxed));
            snapshot.mind[5] = self.mind_ctx.load(Ordering::Relaxed) as f32;
            snapshot.mind[6] = f32::from_bits(self.mind_rate.load(Ordering::Relaxed));
            snapshot.mind[7] = if self.mind_fullattn.load(Ordering::Relaxed) != 0 { 1.0 } else { 0.0 };
            // #554 Tier 1 — the embedded-viewport mirror → the reserved `mindview[3]` slot
            // (no Shared size / LAYOUT_VERSION change; #541 T1 reserved [3..8] for exactly
            // this whole-window state). The visual edge-reads it to create or drop its frame
            // ring. 0 = off = today's behaviour, byte for byte.
            //
            // #609 — the request is a **conjunction**, and the second half is the fix: the
            // editor's window has to still exist. `viewport_on` latches on the first pane
            // draw and never clears (there is no "the pane stopped drawing" event), so on
            // its own it would keep the mirror alive for the life of the process after one
            // editor open. `EguiState::open` is the missing half: `nih_plug_egui` sets it in
            // `Editor::spawn` and clears it in `EguiEditorHandle::drop`, which is exactly
            // "the host closed the window". An `Acquire` load — allocation-free, and the
            // crate's own docs point at it for skipping work while the GUI is shut.
            //
            // #593 Tier 4 — **not stamped at all in the Mind edition.** Mind's editor renders
            // the world itself, so there is nobody to mirror *for*; leaving the slot at its
            // `Shared::default()` zero is the same statement the conjunction below would have
            // reached, made by the compiler instead of at run time.
            #[cfg(not(feature = "mind-edition"))]
            {
                snapshot.mindview[3] = if frame_ring::mirror_requested(
                    self.params.editor_state.is_open(),
                    self.viewport_on.load(Ordering::Relaxed) != 0,
                ) {
                    1.0
                } else {
                    0.0
                };
            }
            // #423 Tier 1 — the atlas runtime block. atlas[0] = gen counter (drives the
            // visual's sidecar re-read + constellation rebuild); atlas[1] = on;
            // atlas[2] = roofline inset on. Rest reserved. All-zero = inert.
            snapshot.atlas[0] = self.atlas_gen.load(Ordering::Relaxed) as f32;
            snapshot.atlas[1] = if self.atlas_on.load(Ordering::Relaxed) != 0 { 1.0 } else { 0.0 };
            snapshot.atlas[2] = if self.atlas_roofline.load(Ordering::Relaxed) != 0 { 1.0 } else { 0.0 };
            // Liquid "reset pool" counter (#182 T3a) — rides the reserved
            // liquid[14] slot (packers leave it 0; presets never capture it).
            snapshot.liquid[14] = self.liq_reset_gen.load(Ordering::Relaxed) as f32;
            // Storyboard "next shot" trigger (#307 Tier 3): a live counter the visual
            // edge-detects to advance at the next bar.
            snapshot.cam_story[4] = self.story_next_gen.load(Ordering::Relaxed) as f32;

            // Live audio band envelopes (pulse_source already rode in via
            // to_shared). The visual reads `audio[1]` (bass) as a pulse source.
            snapshot.audio[..audio::NUM_BANDS].copy_from_slice(&bands);
            // #248 Tier 1: the smoothed broadband RMS level rides audio[5] — the
            // loudness envelope that drives the audio-dipole's amplitude.
            snapshot.audio[audio::NUM_BANDS] = level;
            // #248 Tier 3: the smoothed spectral centroid (0..1 log axis, the
            // pitch/brightness proxy) rides audio[6]; the smoothed stereo balance
            // (−1 L .. +1 R) rides audio[7].
            snapshot.audio[audio::NUM_BANDS + 1] = centroid;
            snapshot.audio[audio::NUM_BANDS + 2] = balance;

            // Live host transport → visual's PLL beat-clock. `pos_beats` is
            // wrapped mod 1024 so it survives the f32 in `Shared` with plenty of
            // phase precision; the visual only uses its fractional part.
            let t = context.transport();
            // Use the HELD host tempo (updated above) so the visual's beat clock keeps
            // following the DAW even across blocks where the host omits the tempo.
            snapshot.transport = [
                if t.playing { 1.0 } else { 0.0 },
                t.pos_beats().map(|b| b.rem_euclid(1024.0) as f32).unwrap_or(0.0),
                self.last_host_bpm as f32,
                if self.last_host_bpm > 0.0 { 1.0 } else { 0.0 },
            ];
            // #354: publish the absolute (unwrapped) beat position to the editor's
            // GUI thread for beat-quantized recalls; `-1.0` when stopped so the
            // editor recalls immediately. Unwrapped so "next boundary" arithmetic
            // needs no wrap handling (f32 has ample precision over a session).
            let beat_now = if t.playing {
                t.pos_beats().map(|b| b as f32).unwrap_or(-1.0)
            } else {
                -1.0
            };
            self.beat_pos.store(beat_now.to_bits(), Ordering::Relaxed);

            // Audio-detected tempo (#307): live BPM + confidence for the camera
            // clock's Audio source. Held through breakdowns inside the estimator.
            snapshot.cam_audio[0] = cam_bpm;
            snapshot.cam_audio[1] = cam_bpm_conf;

            // #333: calibrated meters + RTA (measured this block) → the visual HUD
            // and the in-world instrument (Tier 3).
            snapshot.audiometer = audiometer;
            snapshot.audiospectrum = audiospectrum;

            // #346 Field Chamber: publish the triggered/downsampled oscilloscope
            // display frame from the raw ScopeRing (the visual is a separate process
            // and can't read the ring). Gate on the EFFECTIVE panel state in the
            // snapshot (`chamber[0]` panels_on · `chamber[2]` rear scope) — NOT the raw
            // params — so a held key-map preset with the scope wall on still gets a live
            // trace instead of a zeroed frame (Bugbot). Otherwise `scopewave` stays
            // silent (byte-identical default). No alloc — the read reuses `self.scope_win`.
            if snapshot.chamber[0] > 0.5 && snapshot.chamber[2] > 0.5 {
                const SCOPE_N: usize = 256;
                let sr = self.sample_rate.max(1.0);
                let ch = self.params.panel_scope_channel.value().clamp(0, 2) as usize;
                let trig = self.params.panel_scope_trigger.value().clamp(0, 2) as u32;
                let span = ((self.params.panel_scope_time_ms.value() * sr / 1000.0) as usize)
                    .clamp(16, 4096);
                // span samples to display + a one-span search margin for the trigger.
                let want = if trig == 0 { span } else { span * 2 };
                self.scope.read_recent_mid(ch, want, &mut self.scope_win);
                let mut wave = [0.0f32; SCOPE_N];
                let (n, locked) = audio::scope_frame_into(&self.scope_win, span, &mut wave, trig, 0.0);
                snapshot.scopewave[0] = n as f32;
                snapshot.scopewave[1] = sr;
                snapshot.scopewave[2] = ch as f32;
                snapshot.scopewave[3] = if locked { 1.0 } else { 0.0 };
                snapshot.scopewave[4..4 + SCOPE_N].copy_from_slice(&wave);
            }

            // #339 Tier 1: publish the played-note radiators so the visual can draw
            // what you play (each voice a shell at its lensed visual rate). Runtime
            // block — stamped here, never param-packed. Cheap even when off (all 0).
            if synth_on {
                let cfg = build_synth_config(&self.params, self.synth_bend, level, self.synth_beat, pump_env, 0.0, self.synth_note_hz);
                self.synth.write_voices(&mut snapshot.voices, &cfg);
            }

            // Only publish if no apply started/finished during this capture, so the
            // snapshot is wholly pre- or post-apply, never a mix.
            if self.apply_gen.load(Ordering::Acquire) == apply_gen0 {
                w.write(snapshot);
            }
        }
        // Keep the host from suspending us while we generate sound from silence.
        if synth_on {
            ProcessStatus::KeepAlive
        } else {
            ProcessStatus::Normal
        }
    }
}

/// #593 Tier 1 — everything `editor_ui` needs from the plugin instance.
///
/// `Plugin::editor` used to clone these 43 handles straight into the
/// `create_egui_editor` update closure; they now ride across in one struct so a
/// **second** editor host (Organon Mind's own window, #593 Tier 2) can hand the
/// same body the same things and draw the identical interface. One field per
/// capture, in the order they were cloned, under the names the body already uses.
///
/// `pub(crate)`, not `pub`: three of the field types (`keymap::KeyMap`,
/// `controller::Mailbox`, and — on `editor_ui` — `preset::PresetUi`) live in private
/// modules, and Tier 2's host has to live in this crate anyway (it is returned from
/// `Plugin::editor`). Widening it would mean making those modules public purely to
/// satisfy `private_interfaces` — collateral this change does not need.
pub(crate) struct EditorCtx {
    pub(crate) params: Arc<OrganicMathParams>,
    pub(crate) release: Arc<AtomicBool>,
    pub(crate) hdr_gen: Arc<AtomicU32>,
    /// #593 Tier 4 — absent in the Mind edition, along with the pane that stores into it.
    #[cfg(not(feature = "mind-edition"))]
    pub(crate) viewport_on: Arc<AtomicU32>,
    pub(crate) material_gen: Arc<AtomicU32>,
    pub(crate) beat_pos: Arc<AtomicU32>,
    pub(crate) overlay_gen: Arc<AtomicU32>,
    pub(crate) liq_reset_gen: Arc<AtomicU32>,
    pub(crate) story_next_gen: Arc<AtomicU32>,
    pub(crate) nn_gen: Arc<AtomicU32>,
    pub(crate) creature_gen: Arc<AtomicU32>,
    pub(crate) agent_on: Arc<AtomicBool>,
    pub(crate) chat_gen: Arc<AtomicU32>,
    pub(crate) plan_gen: Arc<AtomicU32>,
    pub(crate) release_gen: Arc<AtomicU32>,
    pub(crate) name_gen: Arc<AtomicU32>,
    pub(crate) model_gen: Arc<AtomicU32>,
    pub(crate) model_readout: Arc<std::sync::Mutex<String>>,
    pub(crate) mind_topo: Arc<AtomicU32>,
    pub(crate) mind_prompt_gen: Arc<AtomicU32>,
    pub(crate) mind_temp: Arc<AtomicU32>,
    pub(crate) mind_ctx: Arc<AtomicU32>,
    pub(crate) mind_rate: Arc<AtomicU32>,
    pub(crate) mind_fullattn: Arc<AtomicU32>,
    pub(crate) mind_console: Arc<std::sync::Mutex<mind_console::MindConsole>>,
    pub(crate) atlas_gen: Arc<AtomicU32>,
    pub(crate) atlas_on: Arc<AtomicU32>,
    pub(crate) atlas_roofline: Arc<AtomicU32>,
    pub(crate) atlas_readout: Arc<std::sync::Mutex<String>>,
    pub(crate) atlas_loaded_profile: Arc<std::sync::Mutex<Option<crate::math::HardwareProfile>>>,
    pub(crate) field_gen: Arc<AtomicU32>,
    pub(crate) nca_gen: Arc<AtomicU32>,
    pub(crate) field_load_pending: Arc<AtomicBool>,
    pub(crate) fieldclip_gen: Arc<AtomicU32>,
    pub(crate) apply_gen: Arc<AtomicU32>,
    pub(crate) keymap: Arc<ArcSwap<keymap::KeyMap>>,
    pub(crate) active_note: Arc<AtomicU8>,
    pub(crate) audio_viz: Arc<audio::AudioViz>,
    pub(crate) scope: Arc<audio::ScopeRing>,
    pub(crate) active_bpm: Arc<AtomicU32>,
    pub(crate) tempo_src_active: Arc<AtomicU32>,
    pub(crate) perf_mailbox: Arc<controller::Mailbox>,
    pub(crate) editor_state: Arc<nih_plug_egui::EguiState>,
    /// #593 Tier 4 — **has this editor's host already drawn the 3-D world into the surface
    /// egui is about to paint on?**
    ///
    /// The one thing `editor_ui` needs to know about its host rather than about the plugin, and
    /// it is here because `EditorCtx` is the only channel into the body. `false` for every host
    /// that owns all of its own pixels: the VST3/CLAP plugin, `organon-standalone`, and
    /// Organon Mind under the `nih_plug_egui` editor. `true` only under `wgpu_editor`, which
    /// runs `World::render_into` on the same surface immediately before egui's pass.
    ///
    /// It decides exactly one thing — whether the central region is painted opaque
    /// ([`theme::workspace_frame`]) — and that one thing is the whole of Tier 4's first half.
    pub(crate) scene_behind: bool,
}

/// #593 Tier 1 — the editor's entire interface, as a function an editor host can call.
///
/// It began as a **pure hoist** of the `create_egui_editor` update closure: the body between
/// the two `#593 T1` markers below was byte-identical to that closure apart from the uniform
/// dedent, checked mechanically by `native/tools/check-editor-extract.py`. **Still do not
/// reflow, re-order or tidy that region** — the reason has not changed, and a diff against any
/// pre-#602 base should still be readable at a glance.
///
/// ⚠️ **It is no longer byte-identical, and #593 Tier 4 is why.** Two deliberate edits: the
/// central region's frame is now `theme::workspace_frame(scene_behind)` instead of
/// `CentralPanel`'s default, and the `#554` mirror pane is `#[cfg(not(mind-edition))]`. So
/// `check-editor-extract.py` reports a diff against a #602-era base, and that diff is the
/// tier — expect exactly those two hunks and nothing else.
///
/// The captures are re-materialized below as locals with their original names and
/// types (43 `Arc` clones per repaint — atomic increments, immeasurable next to the
/// UI itself), which is what lets the body come across untouched.
pub(crate) fn editor_ui(
    cx: &EditorCtx,
    ctx: &egui::Context,
    setter: &ParamSetter,
    state: &mut preset::PresetUi,
) {
    let params = cx.params.clone();
    let release = cx.release.clone();
    let hdr_gen = cx.hdr_gen.clone();
    #[cfg(not(feature = "mind-edition"))]
    let viewport_on = cx.viewport_on.clone();
    let material_gen = cx.material_gen.clone();
    let beat_pos = cx.beat_pos.clone();
    let overlay_gen = cx.overlay_gen.clone();
    let liq_reset_gen = cx.liq_reset_gen.clone();
    let story_next_gen = cx.story_next_gen.clone();
    let nn_gen = cx.nn_gen.clone();
    let creature_gen = cx.creature_gen.clone();
    let agent_on = cx.agent_on.clone();
    let chat_gen = cx.chat_gen.clone();
    let plan_gen = cx.plan_gen.clone();
    let release_gen = cx.release_gen.clone();
    let name_gen = cx.name_gen.clone();
    let model_gen = cx.model_gen.clone();
    let model_readout = cx.model_readout.clone();
    let mind_topo = cx.mind_topo.clone();
    let mind_prompt_gen = cx.mind_prompt_gen.clone();
    let mind_temp = cx.mind_temp.clone();
    let mind_ctx = cx.mind_ctx.clone();
    let mind_rate = cx.mind_rate.clone();
    let mind_fullattn = cx.mind_fullattn.clone();
    let mind_console = cx.mind_console.clone();
    let atlas_gen = cx.atlas_gen.clone();
    let atlas_on = cx.atlas_on.clone();
    let atlas_roofline = cx.atlas_roofline.clone();
    let atlas_readout = cx.atlas_readout.clone();
    let atlas_loaded_profile = cx.atlas_loaded_profile.clone();
    let field_gen = cx.field_gen.clone();
    let nca_gen = cx.nca_gen.clone();
    let field_load_pending = cx.field_load_pending.clone();
    let fieldclip_gen = cx.fieldclip_gen.clone();
    let apply_gen = cx.apply_gen.clone();
    let keymap = cx.keymap.clone();
    let active_note = cx.active_note.clone();
    let audio_viz = cx.audio_viz.clone();
    let scope = cx.scope.clone();
    let active_bpm = cx.active_bpm.clone();
    let tempo_src_active = cx.tempo_src_active.clone();
    let perf_mailbox = cx.perf_mailbox.clone();
    let editor_state = cx.editor_state.clone();
    // #593 Tier 4 — the one fact the body needs about its *host* rather than its plugin: has
    // the world already been drawn into this surface? See `EditorCtx::scene_behind`.
    let scene_behind = cx.scene_behind;

    // #593 T1 BEGIN editor body
    apply_theme(ctx);
    // Build/refresh the recorded-defaults context for this params
    // instance (#131) so the per-slider ⏺/⟲ can find each param's id.
    ensure_ui_defaults(&params);

    // Output-resolution readout: lazily open the visual's feedback
    // channel (created once the visual is running) and format the live
    // render resolution for the Output Resolution card. Repaint so it
    // keeps ticking even without interaction.
    if state.render_feedback.as_ref().map(|r| !r.is_open()).unwrap_or(true) {
        state.render_feedback = Some(crate::ipc::FeedbackReader::open());
    }
    let feedback = state.render_feedback.as_ref().and_then(|r| r.read());
    let res_text = match &feedback {
        Some(f) => format!(
            "● render {}×{}  ({}%)  ·  {:.0} fps",
            f.width,
            f.height,
            (f.scale * 100.0).round() as i32,
            f.fps
        ),
        None => "● render: visual not running".to_string(),
    };
    // Hardware RT (#195 Tier 0): availability + the live TLAS rebuild
    // cost from the visual. Until the visual reports, assume available
    // (don't grey the card out preemptively on a machine that has it).
    let (rt_available, rt_tlas_ms) = match &feedback {
        Some(f) => (f.rt_available != 0, f.tlas_ms),
        None => (true, 0.0),
    };
    // Path tracer (#200 Tier 4): ground-truth mode active + spp.
    let (pathtrace_active, pathtrace_spp) = match &feedback {
        Some(f) => (f.pathtrace_active != 0, f.pathtrace_spp),
        None => (false, 0),
    };
    // Neural radiance cache (#256 Tier 0): live training loss + state.
    let (nrc_loss, nrc_state) = match &feedback {
        Some(f) => (f.nrc_loss, f.nrc_state),
        None => (0.0f32, 0u32),
    };
    // Neural acceleration (#200 Tier 2): adapter support detection.
    let (coopmat_available, f16_available) = match &feedback {
        Some(f) => (f.coopmat_available != 0, f.f16_available != 0),
        None => (false, false),
    };
    // Metal interop island (#200 Tier 3): startup probe result.
    let (island_available, island_gflops) = match &feedback {
        Some(f) => (f.metal_island_available != 0, f.tensor_gflops),
        None => (false, 0.0),
    };
    ctx.request_repaint();

    // #356: drain the performance-controller mailbox before any panel
    // draws, so both the presets rail and the mirror grid see a
    // consistent state this frame. Runs every frame the editor is open.
    perf_controller_drain(
        state,
        &perf_mailbox,
        &apply_gen,
        &params,
        setter,
        &hdr_gen,
        &beat_pos,
        ctx.input(|i| i.time),
    );

    // #317 UI-sync: mirror any agent param changes onto the real sliders /
    // dropdowns so the editor never disagrees with the visual. Every frame the
    // editor is open (which is whenever the agent is used — the chat box is here).
    // Skip while the user is actively dragging a control so a queued op can't
    // overwrite a live nudge (deferred, not dropped).
    agent_apply_drain(state, &params, setter, ctx.is_using_pointer());

    // Performance / diagnostics status bar (#277). Docked to the very
    // bottom and added BEFORE the side/central panels so it spans the
    // full editor width. Toggled by the 📊 Perf header button.
    if state.perf_open {
        // Fixed, compact height — a slim strip sized to the CPU/GPU hero
        // meters + tiles (down from 172; the meters/graph/tiles shrank to
        // match). An unconstrained bottom panel expands to fill, so keep
        // it explicit.
        let bpm = f32::from_bits(active_bpm.load(Ordering::Relaxed));
        let tsrc = tempo_src_active.load(Ordering::Relaxed);
        // ⚠️ **`exact_height` is what made this invisible, not the height value.** The bar
        // was pinned to exactly `PERF_BAR_H` with no way to grow and no way to scroll, so
        // anything the content needed past that was simply *clipped* — silently, with no
        // scrollbar and no clue that there was more. Two things push it over:
        //
        //   * **A host-set DPI scale.** Inside a DAW the editor's `pixels_per_point` comes
        //     from the host, and every row in `perf_bar_ui` grows with it while a constant
        //     100.0 does not. At 150% the meters and tiles no longer fit; at 300% almost
        //     nothing does. That is exactly the configuration this was reported from.
        //   * A window short enough that egui shrinks the panel to fit the screen anyway.
        //
        // So: `resizable` + `default_height` instead of `exact_height` (the same shape the
        // Mind bottom dock 200 lines up already uses), a `min_height` floor so it can never
        // collapse to a sliver, and the body in a `ScrollArea` so content that still does
        // not fit is *reachable* rather than cropped. `PERF_BAR_H` stays the default, so a
        // window with room looks exactly as it did before.
        egui::TopBottomPanel::bottom("perf_status_bar")
            .resizable(true)
            .default_height(PERF_BAR_H)
            .min_height(PERF_BAR_MIN_H)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        perf_bar_ui(ui, feedback.as_ref(), state, bpm, tsrc);
                    });
            });
    }

    // #551 Tier 1 — the UI theme editor. A resizable dock rather than a floating
    // window on purpose: you are judging colours and grain *against* the rest of the
    // interface, so the thing being edited has to stay visible beside the controls
    // editing it. It sits outside the presets rail because it is not a preset.
    if state.ui_theme_open {
        egui::SidePanel::right("ui_theme_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                theme::panel_surface(ui);
                theme_config::ui_panel(ui, &mut state.theme_lib, &mut state.theme_rename);
            });
    }

    // Presets + their management live in a dedicated, resizable right
    // rail so they're always reachable without scrolling past the
    // parameter grid.
    egui::SidePanel::right("presets_panel")
        .resizable(false)
        .exact_width(150.0)
        .show(ctx, |ui| {
            theme::panel_surface(ui);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(2.0);
                ui.heading(egui::RichText::new("Presets").color(theme::BONE()).strong());
                ui.label(
                    egui::RichText::new("Save / recall the full parameter state.")
                        .weak(),
                );
                ui.add_space(6.0);
                presets_ui(
                    ui, &apply_gen, &params, setter, state, &hdr_gen, &beat_pos,
                    &name_gen,
                );
            });
        });

    // #483 Tier 1 — Organon Mind has no Generator tab, so a freshly loaded
    // `.gguf` would light nothing: the specimen only draws when the generator
    // is Neural Network and its topology is Connectome (loaded). Point the
    // visual at it once per model load. This is the GUI thread, so
    // `ParamSetter` is legal (the audio thread could never do this), and it
    // fires on the `model_gen` **edge** so it never fights a later manual
    // change. Full Organon never auto-sets — there you pick your own.
    {
        let gen = model_gen.load(Ordering::Relaxed);
        // #620 — **the latch moves only once the params read back as intended.**
        //
        // It used to be set *before* the writes, which gave the auto-point exactly one attempt per
        // load and no way to retry: lose that attempt and the model stayed un-pointed until it was
        // loaded again. A Mac session on 2026-08-03 hit precisely that — topology stayed
        // `Small-world`, the generator stayed `Original`, so *both* writes were missing — and it
        // did not reproduce across two later runs. Latching on readback deletes the whole class
        // without anyone having to identify what ate the write, which is why it is the fix even
        // though the root cause was never established.
        //
        // 🔍 **One candidate mechanism, recorded as a lead and not as a diagnosis.** In the
        // standalone wrapper `set_parameter` pushes onto a bounded `unprocessed_param_changes`
        // queue and returns a bool; a full queue drops the change, and the `nih_debug_assert!`
        // that would have said so compiles out of a release build. That fits "both writes lost,
        // once, unreproducibly" — but it is a plausible story over one observation, which this
        // project has already been burned by treating as a finding. Do not write it up as the
        // cause without catching it.
        if crate::mind_ui::should_point_at_specimen(
            crate::edition::EDITION,
            gen,
            state.mind_auto_view_gen,
        ) && state.mind_auto_view_pending != Some(gen)
        {
            state.mind_auto_view_pending = Some(gen);
            state.mind_auto_view_attempts = 0;
            // Where the params stood *before* this auto-point wrote anything. Captured here, once,
            // because it is the only moment it is knowable — see the `Abandon` arm below.
            state.mind_auto_view_baseline =
                Some((params.generator.value().core(), params.nw_topology.value()));
        }
        if let Some(pending) = state.mind_auto_view_pending {
            const TARGET: (crate::params::GeneratorMode, crate::params::NeuralTopology) = (
                crate::params::GeneratorMode::NeuralNetwork,
                crate::params::NeuralTopology::Connectome,
            );
            let now = (params.generator.value().core(), params.nw_topology.value());
            // The readback. `set_parameter` queues for the audio thread rather than applying
            // inline, so this is false for a frame or so after every issue — that is `Wait`, not
            // failure.
            let landed = now == TARGET;
            // Did a third party move either param? Only meaningful against the baseline, and only
            // per-param, because the two land separately (`auto_point_externally_changed`).
            let externally_changed = state
                .mind_auto_view_baseline
                .is_some_and(|baseline| {
                    crate::mind_ui::auto_point_externally_changed(now, baseline, TARGET)
                });
            match crate::mind_ui::auto_point_step(
                landed,
                externally_changed,
                state.mind_auto_view_attempts,
            ) {
                crate::mind_ui::AutoPointStep::Confirm => {
                    state.mind_auto_view_gen = pending;
                    state.mind_auto_view_pending = None;
                    state.mind_auto_view_baseline = None;
                }
                crate::mind_ui::AutoPointStep::Abandon => {
                    // Someone moved a param this auto-point was still working on. Stop, and latch
                    // as though it had succeeded, so nothing re-fires for this load.
                    //
                    // **This is what keeps the promise made at the top of this block** — "fires on
                    // the edge so it never fights a later manual change". One-shot writes kept it
                    // for free; the #620 retry does not, and without this arm a dropped write
                    // would leave a ~3 s window in which a user's deliberate choice is silently
                    // overwritten (#628 review). Their choice wins: it is newer, and it is theirs.
                    state.mind_auto_view_gen = pending;
                    state.mind_auto_view_pending = None;
                    state.mind_auto_view_baseline = None;
                }
                crate::mind_ui::AutoPointStep::GiveUp => {
                    // Latch anyway: a retry that never stops is one way to *produce* the reported
                    // symptom rather than fix it. The model stays un-pointed — same as the old
                    // failure — but it says so instead of being silent about it.
                    nih_warn!(
                        "#620: auto-point at the specimen did not take after {} frames \
                         (generator={:?}, topology={:?}). Set generator = Neural Network and \
                         topology = Connectome by hand, and please report this.",
                        state.mind_auto_view_attempts,
                        params.generator.value(),
                        params.nw_topology.value(),
                    );
                    state.mind_auto_view_gen = pending;
                    state.mind_auto_view_pending = None;
                    state.mind_auto_view_baseline = None;
                }
                crate::mind_ui::AutoPointStep::Issue => {
                    setter.begin_set_parameter(&params.generator);
                    setter.set_parameter(
                        &params.generator,
                        crate::params::HostGeneratorMode::NeuralNetwork,
                    );
                    setter.end_set_parameter(&params.generator);
                    setter.begin_set_parameter(&params.nw_topology);
                    setter.set_parameter(
                        &params.nw_topology,
                        crate::params::NeuralTopology::Connectome,
                    );
                    setter.end_set_parameter(&params.nw_topology);
                    state.mind_auto_view_attempts += 1;
                }
                crate::mind_ui::AutoPointStep::Wait => {
                    state.mind_auto_view_attempts += 1;
                }
            }
        }
    }

    // ── #520 Tier 2 — the resizable window ─────────────────────────
    // STANDALONE: baseview creates the `NSWindow` with
    // `Titled | Closable | Miniaturizable` and no `Resizable`, and no
    // API to change it, so we OR the bit in through objc. Called every
    // frame on purpose — it is the first point where the window
    // reliably exists — and latches after the first success. Inert
    // unless a standalone `main()` marked the process, so the plugin
    // build runs this line and does nothing.
    //
    // PLUGIN: the frame belongs to the HOST, so a native resize is not
    // ours to give; the drag corner below requests the size through the
    // plugin API instead.
    crate::window_macos::ensure_resizable(MIN_EDITOR_W, MIN_EDITOR_H);
    // A native resize moves the *window*; nothing in baseview moves the
    // two nested views inside it, and nothing tells egui. This walks
    // that chain — editor view, its `NSOpenGLView`, then baseview's own
    // `Resized` event — so `screen_rect` and the surface being painted
    // on end up agreeing. The full hierarchy and why each step is
    // needed is documented on `sync_editor_view`.
    //
    // It is called every frame and converges: it compares frames and
    // returns immediately when they already match, so the steady state
    // is a couple of geometry reads. Every consumer of `screen_rect` —
    // the docks below, `fixed_columns` — then reflows without knowing
    // any of this happened.
    crate::window_macos::sync_editor_view();

    // ── Organon Mind: the workstation docks (#532 Tier 1) ──────────
    // Tier 1 builds the workstation *inside the editor window* rather than
    // in a window of its own: `nih_export_standalone` owns the event loop,
    // and for Organon Mind the editor rectangle already **is** the whole
    // window. Two of the five regions therefore existed as chrome already —
    // the presets rail is the right dock, the perf strip is the status bar —
    // so this adds the two that were missing and sizes them with
    // `mind_shell::egui_docks`, which enforces the rule that the furniture
    // yields before the middle does.
    //
    // The docks are drawn for **every** Mind tab, and that is the point:
    // today, switching to Look or Motion hides which model is loaded and
    // whether it is streaming. A workstation keeps those in view while you
    // work on something else.
    //
    // Full Organon is untouched: it keeps the Mind lane as one tab among
    // eight with the dashboard inline, so nothing moves for that product.
    // Did the bottom dock actually draw? On a window too small for it the dock
    // is dropped, and the Mind tab falls back to rendering the dashboard
    // inline so the telemetry never disappears entirely.
    let mut mind_bottom_dock_shown = false;
    if crate::edition::EDITION.is_mind() {
        // `screen_rect`, not `available_rect`: `egui_docks` accounts for the
        // rail and the status strip itself, so handing it the already-reduced
        // rect would subtract them twice.
        let screen = ctx.screen_rect();
        let docks = crate::mind_shell::egui_docks(
            screen.width(),
            screen.height(),
            if state.perf_open { PERF_BAR_H } else { 0.0 },
        );
        // One tick per frame, before anything reads `mind_viz`.
        mind_observe(ctx, state);
        if docks.bottom > 0.0 {
            mind_bottom_dock_shown = true;
            egui::TopBottomPanel::bottom("mind_readouts")
                .resizable(true)
                .default_height(docks.bottom)
                .show(ctx, |ui| {
                    theme::panel_surface(ui);
                    // Scrolled: the dashboard's full (non-compact) height
                    // exceeds the default dock, and clipping a readout is
                    // worse than making it reachable.
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        mind_dashboard_ui(ui, state);
                    });
                });
        }
        if docks.left > 0.0 {
            egui::SidePanel::left("mind_model_dock")
                .resizable(false)
                .exact_width(docks.left)
                .show(ctx, |ui| {
                    theme::panel_surface(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.add_space(2.0);
                        ui.heading(
                            egui::RichText::new("Model").color(theme::BONE()).strong(),
                        );
                        if ui
                            .button("Load AI Model… (.gguf)")
                            .on_hover_text(
                                "Parse a GGUF model's header (metadata + \
                                 tensor directory only — no weights loaded). \
                                 Opens at ~/.lmstudio/models/.",
                            )
                            .clicked()
                        {
                            pick_model_async(
                                model_gen.clone(),
                                model_readout.clone(),
                            );
                        }
                        let readout = model_readout
                            .lock()
                            .map(|s| s.clone())
                            .unwrap_or_default();
                        if readout.is_empty() {
                            ui.label(
                                egui::RichText::new("No model loaded.")
                                    .weak()
                                    .small(),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new(readout).monospace().small(),
                            );
                        }
                        ui.separator();
                        // Whether telemetry is actually arriving. This is the
                        // question the dock exists to answer from any tab:
                        // a flat dashboard means "idle", not "broken", and
                        // the two are indistinguishable without saying so.
                        let (dot, text) = if state.mind_viz.active {
                            (ACCENT(), "streaming")
                        } else {
                            (egui::Color32::GRAY, "idle — no tokens arriving")
                        };
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("●").color(dot));
                            ui.label(egui::RichText::new(text).weak().small());
                        });
                    });
                });
        }
    }

    // `ResizableWindow` wraps the `CentralPanel` and draws a drag
    // corner, writing the dragged size back through
    // `EguiState::set_requested_size` — which `EguiState` persists, so
    // the size the user picks is saved with plugin state. This is the
    // *only* resize affordance in a plugin, where the host owns the
    // frame; on the standalone it sits alongside the native edge-drag.
    //
    // Note the corner lands at the CentralPanel's corner, which with
    // the Mind docks shown is the inner corner of the middle region
    // rather than the window's. That is the honest place for it — it
    // resizes the window, and the middle is what grows — but it is why
    // the standalone also wants the native affordance, which is
    // unambiguously the whole window's.
    //
    // The three-column grid needs nothing either way: `fixed_columns`
    // already divides `available_width`, so the columns widen on their
    // own the moment the window can.
    // #554 Tier 1 — the viewport is drawn **inside** the editor body, directly under
    // the tab bar (see the `tab_bar` call below), NOT here.
    //
    // It was a `TopBottomPanel::top(...).show(ctx, ..)`, which is a *Context-level*
    // panel: egui gives those the window edge, outside and above the `CentralPanel`.
    // Since the heading, the button row and the tab bar all live inside that central
    // panel, a ctx-level top panel can only ever land above the product title — which
    // is exactly where it appeared on the Mac. Placement is not a matter of
    // declaration order among panels; it is which surface the panel is registered on.
    //
    // The mirror publishes on its own clock, so the editor keeps asking rather than
    // waiting to be told. Unconditional now that the viewport is always present.
    ctx.request_repaint();

    ResizableWindow::new("organon-editor-resize")
        .min_size(egui::Vec2::new(MIN_EDITOR_W as f32, MIN_EDITOR_H as f32))
        // #593 Tier 4 — **half one of the tier, in one line.** `ResizableWindow` wraps a
        // `CentralPanel`, and `CentralPanel::default()` fills its whole rect with
        // `Visuals::panel_fill`. Under Mind's wgpu editor the world has already been rendered
        // into that exact surface, so an opaque fill does not sit *behind* the interface — it
        // paints the scene out. `None` here is today's faceplate for every other host, so full
        // Organon is untouched; `Some(transparent)` is what makes Mind's central region the
        // viewport rather than a picture of one. See `theme::workspace_frame`.
        // #617 Tier 1 — the transparent frame is now **immersive mode**, not "we are under the
        // wgpu editor". `scene_behind` still gates it, because a host that owns all of its own
        // pixels has nothing behind the panel to reveal; the mode chooses between the two shapes
        // that host can take.
        .frame(theme::workspace_frame(scene_behind && state.immersive))
        .show(ctx, editor_state.as_ref(), |ui| {
        // ── #621 — the camera's interaction region, immersive arm ───────────────────────
        //
        // **Registered here, before the interface, and only in immersive mode.** The scene is
        // the whole window under that mode, so the region is the whole central rect — and it has
        // to go *under* everything egui draws, because egui breaks a hit-test tie by taking the
        // topmost widget. Every card, slider and button below is registered afterwards and so
        // beats it; the gutters between them, which is where the scene actually shows, do not.
        // `scene_input::press_belongs_to_the_scene` carries the second half of that rule.
        //
        // Workstation's arm is at the pane, ~250 lines down — it is the same call with a
        // different rect and a different registration order, and the two are exclusive.
        let scene_mode = crate::scene_input::SceneMode::from_immersive(state.immersive);
        if scene_behind && state.immersive {
            let rect = ui.max_rect();
            crate::scene_input::scene_viewport(ui, rect, scene_mode, &mut state.scene_input);
        }
        // `drag_to_scroll` off wherever a scene is behind the panel (#621). Dragging the
        // background of a window whose background *is* the world means orbit, not scroll — and
        // in immersive mode the scroll area's own drag-to-scroll widget would otherwise be
        // registered after the region above and win every tie against it. It is a touch
        // affordance an instrument on a desk does not need. Full Organon (`scene_behind` false)
        // keeps it, unchanged.
        let mut scroll_source = egui::containers::scroll_area::ScrollSource::ALL;
        scroll_source.drag = !scene_behind;
        egui::ScrollArea::vertical().scroll_source(scroll_source).show(ui, |ui| {
            // Reset All, right-aligned. **Both editions** used to open with a product heading
            // and a hint line under it; both are gone.
            //
            // The heading was the product name a second time — every window that draws this
            // already has the name in its title bar, and in the host the device is labelled
            // anyway. The hint ("automatable / MIDI-mappable by the host") was additionally
            // *false* in Organon Mind, which is standalone-only and has no host. Under Mind's
            // wgpu editor both also sat directly on the scene, over a backdrop with no stable
            // colour, which is where a typography preference became a legibility defect.
            //
            // The row survives them because Reset All lives in it.
            ui.horizontal(|ui| {
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .button("⟲ Reset All")
                            .on_hover_text("Reset every parameter to its default")
                            .clicked()
                        {
                            reset_all(&apply_gen, &params, setter);
                        }
                    },
                );
            });
            ui.horizontal(|ui| {
                if ui.button("◼  Open Visual Window").clicked() {
                    spawn_visual();
                }
                if ui
                    .selectable_label(state.keymap_open, "🎹 Key Map")
                    .on_hover_text("Assign presets to MIDI notes — hold a key to make its preset active")
                    .clicked()
                {
                    state.keymap_open = !state.keymap_open;
                }
                if ui
                    .selectable_label(state.perf_window_open, "🎛 Controller")
                    .on_hover_text("Four-Quadrant Performance Controller (#356): drive beat-quantized component recalls from a Launchpad-style pad grid")
                    .clicked()
                {
                    state.perf_window_open = !state.perf_window_open;
                }
                if ui
                    .selectable_label(state.tab == preset::UiTab::Audio, "🎵 Audio")
                    .on_hover_text("Audio instrument: input meter, spectrum, calibrated BS.1770 meters + RTA, and the oscilloscope")
                    .clicked()
                {
                    state.tab = preset::UiTab::Audio;
                }
                if ui
                    .selectable_label(state.perf_open, "📊 Perf")
                    .on_hover_text("Performance status bar: live frame rate, frame-load meter, and diagnostics")
                    .clicked()
                {
                    state.perf_open = !state.perf_open;
                }
                // #554 Tier 1 — there is deliberately NO viewport toggle. The viewport
                // is native to the window, not a panel you opt into: the instrument
                // has simply never had one until now. It shipped behind a `◱ Viewport`
                // selectable, which framed a missing feature as an optional extra.
                // That has not changed, and #609 did not add one.
                //
                // ⚠️ The `viewport_on.store(1, …)` that used to sit here has moved to the
                // pane's own draw site, next to `viewport_pane`. Requesting the mirror
                // from the *button row* meant the request survived every later change to
                // whether the pane drew at all — including the #593 Tier 4 gate, which
                // will stop Mind drawing it. The signal now originates where the thing
                // being signalled about actually happens.
                if ui
                    .selectable_label(state.ui_theme_open, "◐ UI")
                    .on_hover_text(
                        "Interface theme: colour pickers for every token, plus the \
                         grain, gradient, bevel and lighting treatments. Saved \
                         separately from parameter presets — recalling a Scene never \
                         restyles the editor.",
                    )
                    .clicked()
                {
                    state.ui_theme_open = !state.ui_theme_open;
                }
                // #617 Tier 1 — the view-mode switch. Only offered where it means something: a
                // host that owns all of its own pixels (the plugin, `organon-standalone`,
                // Mind under the egui editor) has no scene behind the panel, so there is nothing
                // for immersive mode to reveal and the control would be a lie.
                if scene_behind
                    && ui
                        .selectable_label(state.immersive, "⛶ Immersive")
                        .on_hover_text(
                            "Let the scene fill the window with the interface floating over it. \
                             Off: the scene is a viewport pane laid out with the workstation. \
                             A view mode, not a parameter — presets never change it.",
                        )
                        .clicked()
                {
                    state.immersive = !state.immersive;
                }
                if ui
                    .button("Release MIDI clip")
                    .on_hover_text("Hand control back to the sliders after a clip")
                    .clicked()
                {
                    release.store(true, Ordering::Relaxed);
                }
                if ui
                    .button("Open HDR Environment…")
                    .on_hover_text("Load a .hdr equirectangular environment map")
                    .clicked()
                {
                    pick_hdr_async(hdr_gen.clone());
                }
            });
            ui.add_space(6.0);

            // Three columns of cards: geometry/motion, animation/pulse,
            // and the look (material/lighting/post). The usable row
            // width `w` is measured from each column here — once, at
            // the top, before the card frame + collapsing header nest
            // and pollute `available_width()` — then threaded into the
            // rows so sliders size to the column and never overflow it.
            // Hoisted generator-mode classification — used by several
            // tabs (the Generator tab's Surface card + the Look tab's
            // KIFS gating), so it's computed once before the tab bar.
            use crate::params::GeneratorMode;
            let gmode = params.generator.value().core();
            let original = gmode == GeneratorMode::Original;
            // Minimal-surface is dual-path: parametric families emit a
            // (u,v) Grid that Surface modes skin — Weierstrass (3..5) and
            // the CMC surfaces of revolution (13..14) — while the implicit
            // families (TPMS 0..2, bubbles/foam 6,7, algebraic 8..12)
            // raymarch. So it's only "node-free" when implicit.
            let msf = params.ms_family.value().to_u32();
            let ms_parametric = gmode == GeneratorMode::MinimalSurface
                && ((3..=5).contains(&msf) || (13..=14).contains(&msf));
            // Generators with no node field have no Surface mode / palette
            // (Mandelbulb, KIFS, Minimal-surface's implicit families); KIFS
            // is additionally a self-contained fullscreen colour field, so
            // its node/PBR-surface look cards don't apply either.
            // Neural field is dual-path like Minimal-surface: the Raymarch
            // form is node-free (raymarch), the Strand form (#200 T1b) builds
            // a node field, so the Surface cards apply.
            let neural_ray = gmode == GeneratorMode::NeuralField
                && !params.neural_strands_mode.value();
            let raymarch = matches!(
                gmode,
                GeneratorMode::Mandelbulb
                    | GeneratorMode::Kaleidoscope
                    | GeneratorMode::Lens
                    | GeneratorMode::Creature
            ) || (gmode == GeneratorMode::MinimalSurface && !ms_parametric)
                || neural_ray;
            let kifs = gmode == GeneratorMode::Kaleidoscope;

            // Top-level tabs (Blender-style): show one section at a
            // time rather than three crowded columns side by side.
            // Environment (the world layer, was the 🌍 panel) and
            // Settings (per-display plumbing + capture, was the 🎬
            // panel) are UI-only tabs — no per-tab presets (`UiTab`).
            // #483 Tier 1: the bar is edition-filtered — full Organon gets
            // all eight in this order; Organon Mind gets the Mind lane +
            // Settings, and an active-but-hidden tab is clamped so the
            // window can never come up blank. See `mind_ui::tab_bar`.
            crate::mind_ui::tab_bar(ui, &mut state.tab);
            ui.separator();

            // ── #554 Tier 1 — the embedded viewport, directly under the tab bar ──
            //
            // #593 TIER 4 GATED THIS OUT OF ORGANON MIND, and that gate is the tier.
            //
            // What is below is a **photograph**: the visual process renders a second,
            // offscreen 640×360 frame, reads it back over shared memory, and this pane
            // uploads it as a texture at ~15 Hz. It is full Organon's only viewport path —
            // inside Ableton the editor does not own its window, so a GPU device must not
            // enter the host's process — and there it stays, unchanged, mirror and all.
            //
            // In Mind it was drawing *over* a world `wgpu_editor` was already rendering into
            // this very surface at full rate, unseen. So the pane goes, the mirror request
            // goes with it (there is no `viewport_on` in a mind-edition build to store into),
            // and what shows in this space instead is the scene itself — because the frame
            // this `CentralPanel` draws in is transparent under that host. Two halves of one
            // change: `theme::workspace_frame` above opens the hole, this closes the pane
            // that was plugging it.
            //
            // Deletion was #593's original word for this and it was wrong; `MIND_ARCHITECTURE.md`
            // §2.5 has the per-item verdict and why.
            //
            // The space is **reserved with `allocate_exact_size`, and the pane is then
            // handed that exact rect.** Both halves matter, and getting either wrong
            // produced one of the two failures seen on the Mac:
            //
            // - `allocate_ui` does NOT reserve its `desired_size`; it shrinks the
            //   allocation to what the child actually *used*. `viewport_pane` never
            //   allocates — it reads `ui.max_rect()` and draws through `ui.painter()` —
            //   so a paint-only pane used nothing and nothing was reserved. The cards
            //   then laid out as though the viewport were not there and it painted
            //   underneath them. `allocate_exact_size` advances the cursor
            //   unconditionally, so the rest of the tab flows *below* the viewport.
            // - Because the pane trusts `ui.max_rect()`, that rect has to be the one we
            //   intend. Inherited from the surrounding ui it differs per edition — the
            //   Mind docks constrain the middle, full Organon does not — which is why
            //   the same code drew a tall overpainting image in one and a ~12 pt strip
            //   in the other: the letterbox fits into whatever height it is given. A
            //   child ui pinned to the reserved rect makes it the same in both.
            //
            // 16:9 to match `MIRROR_W`×`MIRROR_H` so the letterbox has nothing to
            // letterbox in the common case, clamped so it can neither vanish nor eat
            // the window.
            // Capped against the *window*, not just the width: at the default 1280×860
            // a flat 420 pt would hand the viewport half the editor and leave the cards
            // more cramped than #525 just finished un-cramping. `content_rect` rather
            // than `available_height` because we are inside a `ScrollArea`, whose
            // available height is effectively unbounded and so tells us nothing.
            #[cfg(not(feature = "mind-edition"))]
            // ⚠️ **The PLUGIN draws no viewport — controls only, as the original did.**
            // The separate visual window is the plugin's only picture, deliberately: in a
            // host the editor is a panel among many, and a second live render of the same
            // scene inside it competes with the projector feed for GPU time while telling
            // you nothing the real window is not already showing bigger.
            //
            // The standalone keeps it. There the editor *is* the app, so a mirror is the
            // difference between a window of sliders and something you can judge a look
            // from, and nothing else is on screen to compete with.
            //
            // `is_standalone()` (not a `cfg`) because one binary cannot answer this at
            // compile time: the plugin cdylib and `organon-standalone` are the same
            // `editor_ui`, and the flag is set by `standalone.rs`'s `main()` before
            // nih-plug starts — the plugin path never sets it. It is a plain `AtomicBool`,
            // so this is portable despite the module's macOS-shaped name.
            if crate::window_macos::is_standalone() {
                let vp_w = ui.available_width();
                let win_h = ui.ctx().content_rect().height();
                let vp_h = (vp_w * crate::frame_ring::MIRROR_H as f32
                    / crate::frame_ring::MIRROR_W as f32)
                    .min(win_h * 0.40)
                    .clamp(160.0, 420.0);
                let (vp_rect, _) =
                    ui.allocate_exact_size(egui::vec2(vp_w, vp_h), egui::Sense::hover());
                let mut vp_ui = ui.new_child(egui::UiBuilder::new().max_rect(vp_rect));
                // #609 — ask for the mirror HERE, at the pane that consumes it, not from the
                // button row above. `process()` ANDs this with `EguiState::is_open()`, so the
                // pair reads as "an editor is open AND this build draws a mirror pane" — and
                // #593 Tier 4 is that gate arriving: this store went with the pane, so Mind
                // stopped requesting a mirror without anyone having to remember to.
                //
                // Keeping the store inside this `if` is what makes the change above pay for
                // itself rather than merely hide something: with the pane gone, the plugin
                // never sets `viewport_on`, so `process()` stops requesting the mirror and
                // the visual stops doing a 640×360 readback for a pane nobody draws.
                viewport_on.store(1, Ordering::Relaxed);
                viewport_pane(&mut vp_ui, state);
                ui.separator();
            }

            // ── #617 Tier 1 — the workstation viewport, directly under the tab bar ──
            //
            // Organon Mind's replacement for the pane above, and deliberately the same *shape*:
            // same 16:9, same clamps, same `allocate_exact_size` so the cards flow below it
            // rather than under it. What differs is what fills it — that pane uploads a CPU
            // photograph at ~15 Hz, this paints a texture `wgpu_editor` rendered the world into
            // on the GPU, at the pane's own Retina resolution and the window's full rate.
            //
            // Reserved only when a host is really drawing a scene (`scene_behind`) **and** the
            // mode is workstation. Immersive reserves nothing: there the world owns the whole
            // window and the interface floats over it, which is #593 Tier 4's arrangement kept
            // intact rather than reverted.
            //
            // 9:16 spelled literally because `frame_ring` — where `MIRROR_W`/`MIRROR_H` live —
            // does not exist in a mind-edition build.
            if scene_behind && !state.immersive {
                let vp_w = ui.available_width();
                let win_h = ui.ctx().content_rect().height();
                let vp_h = (vp_w * 9.0 / 16.0).min(win_h * 0.40).clamp(160.0, 420.0);
                let (vp_rect, _) =
                    ui.allocate_exact_size(egui::vec2(vp_w, vp_h), egui::Sense::hover());
                // Published for the host, which reads it on the NEXT frame — the scene must be
                // drawn before the interface that reserves its rect runs. The one-frame lag is
                // inherent; `PresetUi::scene_pane_rect` carries why it is also harmless.
                state.scene_pane_rect = Some(vp_rect);
                // ── #621 — the camera's interaction region, workstation arm ────────────
                //
                // The pane is the region, so this is registered **here**: after the scroll
                // area that contains it, which is what makes the pane win a tie against the
                // scroll area's background, and after every card above it, which is what
                // makes a drag that started on one of those not reach the camera. The rect is
                // `vp_rect` — the one `allocate_exact_size` just reserved — so the region and
                // the image below it are the same pixels by construction, whatever the
                // workstation has been scrolled to.
                //
                // ⚠️ **`allocate_exact_size` senses `hover()` and that is not an oversight.**
                // A second drag-sensing widget on the same pixels would be a tie for the
                // hit-test to break; the region is the one that senses drags.
                crate::scene_input::scene_viewport(
                    ui,
                    vp_rect,
                    scene_mode,
                    &mut state.scene_input,
                );
                match state.scene_texture {
                    Some(tex) => {
                        ui.painter().image(
                            tex,
                            vp_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    // The first frame after open, and after any resize that rebuilt the texture.
                    // Black rather than a theme fill: this is a viewport with no frame yet, not a
                    // panel, and it is gone by the next repaint.
                    None => {
                        ui.painter().rect_filled(vp_rect, 0.0, egui::Color32::BLACK);
                    }
                }
                // ── "no model loaded" is not "broken" ──────────────────────────────
                //
                // Organon Mind ships no Generator tab by design (#483 T1), so until a `.gguf`
                // load auto-points the generator at Neural Network + Connectome, this viewport
                // shows the **Original** cube field — a perfectly good render of the wrong
                // thing, with nothing on screen saying why. That has read as a regression twice
                // and cost real time both times.
                //
                // ⚠️ **It got easier to misread, not harder, once the viewport became the
                // default** (#593 close-out). Before, an empty-looking Mind was ambiguous
                // between "no model" and "no viewport"; now the viewport is always there, so a
                // cube field is the *only* thing a confused first-run user sees — and it looks
                // exactly like the wgpu editor having failed. One line removes the whole
                // ambiguity.
                //
                // Drawn over the scene rather than in place of it: the render is real and worth
                // seeing, this only says what it is. Its own translucent plate, because #617
                // Tier 2's hard rule applies to anything over a scene whose brightness we do
                // not control — weak-weighted text on an unknown backdrop is unreadable.
                if model_gen.load(Ordering::Relaxed) == 0 {
                    let pad = egui::vec2(10.0, 6.0);
                    let text = "No model loaded — this is the default scene. \
                                Use “Load AI Model…” in the Mind tab.";
                    let galley = ui.painter().layout(
                        text.to_owned(),
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_gray(235),
                        vp_rect.width() - pad.x * 4.0,
                    );
                    let plate = egui::Rect::from_min_size(
                        vp_rect.left_bottom() + egui::vec2(pad.x, -(galley.size().y + pad.y * 3.0)),
                        galley.size() + pad * 2.0,
                    );
                    ui.painter().rect_filled(
                        plate,
                        4.0,
                        egui::Color32::from_black_alpha(190),
                    );
                    ui.painter()
                        .galley(plate.min + pad, galley, egui::Color32::from_gray(235));
                }
                ui.separator();
            }

            // ── Mind tab (#317 AI Performer / #367 Specimen) ──
            // A1 (#317) owns col 0 "Chat / Agent"; B1 (#367) owns col 1
            // "Model / Specimen" (added by that PR — see the build contract).
            if state.tab == preset::UiTab::Mind {
                // Seed the endpoint/model buffers from the sidecar once.
                if !state.agent_cfg_loaded {
                    let cfg = crate::agent::AgentConfig::load();
                    state.agent_endpoint = cfg.endpoint;
                    state.agent_model = cfg.model;
                    state.agent_cfg_loaded = true;
                }
                // #520 Tier 1 — the Organon Mind workstation layout. ONE three-column
                // grid (these cards used to be three stacked `fixed_columns` blocks, which
                // is why they never sat side by side):
                //   col 0  Neural Network  — the generator itself; Mind is always a neural
                //                            network, so its controls lead the tab rather
                //                            than living a tab away. Shared with the
                //                            Generator tab (`neural_network_card`).
                //   col 1  Model / Specimen
                //   col 2  Chat / Agent, then Design Space (atlas)
                // The #482 live-telemetry dashboard follows below, spanning the width.
                fixed_columns(ui, |c| {
                    let w0 = (c[0].available_width() - COL_PAD).max(150.0);
                    neural_network_card(&mut c[0], w0, &params, setter, &nn_gen);
                    card(&mut c[1], "Model / Specimen", |ui| {
                        ui.label(egui::RichText::new(
                            "Load a local LLM as an anatomical specimen — its \
                             architecture becomes a topology you can fly through. \
                             No inference (that's Tier 2); this shows the model's \
                             true form: layers strung along the residual backbone, \
                             attention heads + MLP blocks per layer.",
                        ).weak().small());
                        // #532 Tier 1: Organon Mind carries this button in the
                        // left Model dock, where it is reachable from every
                        // tab. Showing it here as well would put two identical
                        // Load buttons on screen at once, so the dock owns it
                        // in that edition. The card keeps everything else.
                        if !crate::edition::EDITION.is_mind()
                            && ui
                                .button("Load AI Model… (.gguf)")
                                .on_hover_text(
                                    "Parse a GGUF model's header (metadata + tensor \
                                     directory only — no weights loaded). Opens at \
                                     ~/.lmstudio/models/. Then select the \
                                     Neural Network generator + topology = \
                                     'Connectome (loaded)' to see the specimen.",
                                )
                                .clicked()
                        {
                            pick_model_async(model_gen.clone(), model_readout.clone());
                        }
                        let readout = model_readout.lock().map(|s| s.clone()).unwrap_or_default();
                        let model_loaded = !readout.is_empty();
                        if readout.is_empty() {
                            ui.label(egui::RichText::new("No model loaded.").weak().small());
                        } else {
                            ui.label(egui::RichText::new(readout).monospace().small());
                        }
                        // #483 Tier 1 — say what the edition just did on your
                        // behalf. Organon Mind auto-points the visual at the
                        // specimen on load (it ships no generator picker), and
                        // silent state changes are exactly what an instrument
                        // shouldn't have.
                        if crate::edition::EDITION.is_mind() && model_loaded {
                            ui.label(
                                egui::RichText::new(
                                    "view: Neural Network · Connectome (loaded) \
                                     — set automatically on load.",
                                )
                                .weak()
                                .small(),
                            );
                        }
                        ui.separator();
                        // #520 — the mind VIEW (`Shared.mind[2]`): which
                        // geometry the loaded model draws as. 0 = the #367 Tier 1
                        // architecture specimen (default), 1 = the #507 Tier 1
                        // embedding galaxy.
                        //
                        // There is deliberately no "Live" entry. Live was never a
                        // different view — it lit the SAME geometry from the
                        // activation ring — so putting it in this selector forced
                        // a data-source choice through a geometry control, and
                        // Generate had to yank the view to mode 1 to work. Now
                        // frames arriving ARE live: the specimen glows per token
                        // whenever the ring is being written, and sits static when
                        // it isn't.
                        let mut view = mind_topo.load(Ordering::Relaxed).min(1);
                        let was = view;
                        ui.label(egui::RichText::new("view").weak().small());
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut view, 0, "Specimen")
                                .on_hover_text(
                                    "The model's architecture from the GGUF header \
                                     alone: layers along the residual backbone, \
                                     attention-head rings, MLP blocks. Real, exact, \
                                     no projection. Lights up per token while the \
                                     model generates.",
                                );
                            ui.selectable_value(&mut view, 1, "Galaxy")
                                .on_hover_text(
                                    "The vocabulary embedding matrix read out of the \
                                     .gguf, dequantized and projected to 3-D — one \
                                     point per token, brightness = the token's full \
                                     N-D embedding length. Static, no inference. \
                                     Reads ~20k sampled token rows the first time, \
                                     so expect a one-off pause.",
                                );
                        });
                        if view != was {
                            mind_topo.store(view, Ordering::Relaxed);
                            crate::mind_log::append(
                                crate::mind_log::MindEvent::Note,
                                "mind",
                                if view == 1 { "view: embedding galaxy" } else { "view: architecture specimen" },
                            );
                        }
                        // Honesty label (PRD §4 principle 1): a projection is always
                        // presented AS a projection. 2048 dimensions do not fit in 3;
                        // the galaxy is the best available shadow of the real matrix,
                        // never the space itself.
                        if view == 1 {
                            ui.label(egui::RichText::new(
                                "Galaxy = a 3-D projection (PCA) of the N-D embedding \
                                 space — a shadow, not the space. The points and their \
                                 distances are real numbers from the file; the two \
                                 thousand-odd dimensions the picture drops are not \
                                 recoverable from it. It does not animate during \
                                 generation yet — the ring carries per-layer \
                                 activations, not per-token positions.",
                            ).weak().small());
                        }
                        // Node size / extent live on the Neural Network card, which
                        // leads column 0 of this tab as of #520 Tier 1.
                        ui.label(egui::RichText::new(
                            "Sizing (node size, extent) is on the Neural Network card.",
                        ).weak().small());

                        // #367 Tier 2b — embedded llama.cpp runtime: type a
                        // prompt, run REAL inference, tap per-token activations.
                        ui.separator();
                        ui.label(egui::RichText::new(
                            "— live inference (Tier 2b) —",
                        ).weak().small());
                        ui.label(egui::RichText::new(
                            "Runs a real model inside Organon and lights the network \
                             from its per-token activations. Requires the \
                             `organic-math-mind-runtime` helper running (built with \
                             `cargo build --release --features embedded-llm`) and a model \
                             loaded above. The specimen lights up per token on its own.",
                        ).weak().small());
                        ui.add(
                            egui::TextEdit::multiline(&mut state.mind_prompt_input)
                                .desired_rows(2)
                                .hint_text("Prompt the model…")
                                .desired_width(f32::INFINITY),
                        );
                        if ui
                            .button("Generate")
                            .on_hover_text(
                                "Write the prompt to organic-math-mind-prompt.txt and \
                                 bump prompt_gen; the embedded runtime streams the \
                                 reply + per-token activations. The specimen lights \
                                 up per token on its own — the view is not changed.",
                            )
                            .clicked()
                            && !state.mind_prompt_input.trim().is_empty()
                        {
                            let prompt = state.mind_prompt_input.clone();
                            if std::fs::write(
                                ipc::mind_prompt_path(),
                                prompt.as_bytes(),
                            )
                            .is_ok()
                            {
                                // Fresh reply readout for this run.
                                let _ = std::fs::write(ipc::mind_reply_path(), b"");
                                state.mind_reply.clear();
                                mind_prompt_gen.fetch_add(1, Ordering::Relaxed);
                                // #520 — Generate does NOT touch the view. The glow
                                // follows the ring on its own; yanking the selector
                                // made Galaxy unusable (you could never watch a run
                                // from it because Generate switched you away).

                                // If the console runtime is running, trigger it directly
                                // over stdin too (the prompt is already in the sidecar) —
                                // this fires even with the transport stopped, unlike the
                                // audio-thread-stamped prompt_gen counter above.
                                if let Ok(mut console) = mind_console.lock() {
                                    if console.is_running() {
                                        console.send_command("gen");
                                    }
                                }
                                crate::mind_log::append(
                                    crate::mind_log::MindEvent::Prompt,
                                    "mind-runtime",
                                    prompt.trim(),
                                );
                            }
                        }
                        // Runtime dials → mind[4..8] (stamped in process()).
                        let mut temp = f32::from_bits(mind_temp.load(Ordering::Relaxed));
                        if ui
                            .add(egui::Slider::new(&mut temp, 0.0..=2.0).text("temperature"))
                            .changed()
                        {
                            mind_temp.store(temp.to_bits(), Ordering::Relaxed);
                        }
                        let mut ctx_len = mind_ctx.load(Ordering::Relaxed).clamp(256, 32768);
                        if ui
                            .add(
                                egui::Slider::new(&mut ctx_len, 256..=8192)
                                    .text("context")
                                    .logarithmic(true),
                            )
                            .changed()
                        {
                            mind_ctx.store(ctx_len, Ordering::Relaxed);
                        }
                        let mut rate = f32::from_bits(mind_rate.load(Ordering::Relaxed));
                        if ui
                            .add(
                                egui::Slider::new(&mut rate, 0.0..=60.0)
                                    .text("tokens/sec (0 = max)"),
                            )
                            .changed()
                        {
                            mind_rate.store(rate.to_bits(), Ordering::Relaxed);
                        }
                        let mut fullattn = mind_fullattn.load(Ordering::Relaxed) != 0;
                        if ui
                            .checkbox(&mut fullattn, "full attention (per-head tap)")
                            .on_hover_text(
                                "Flash-attention OFF so the per-head attention tap can \
                                 read weights (on-Mac cb_eval refinement).",
                            )
                            .changed()
                        {
                            mind_fullattn.store(fullattn as u32, Ordering::Relaxed);
                        }
                        // Reply readout — poll the runtime's reply sidecar.
                        if let Ok(r) = std::fs::read_to_string(ipc::mind_reply_path()) {
                            if !r.is_empty() {
                                state.mind_reply = r;
                            }
                        }
                        if !state.mind_reply.is_empty() {
                            ui.separator();
                            ui.label(egui::RichText::new("reply:").weak().small());
                            egui::ScrollArea::vertical()
                                .max_height(120.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&state.mind_reply)
                                            .monospace()
                                            .small(),
                                    );
                                });
                        }

                        // #367 Tier 2 UX — the in-plugin runtime console. Start/Stop
                        // the embedded organic-math-mind-runtime (no terminal), see
                        // its log live, and drive it via the stdin command REPL. The
                        // REPL trigger bypasses the audio-thread counters, so `gen`
                        // works with the transport stopped.
                        ui.separator();
                        ui.label(egui::RichText::new("— runtime console —").weak().small());
                        let mut do_start = false;
                        if let Ok(mut console) = mind_console.lock() {
                            console.poll_liveness();
                            let running = console.is_running();
                            ui.horizontal(|ui| {
                                if running {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(80, 200, 120),
                                        egui::RichText::new("● running").small(),
                                    );
                                    if ui.button("Stop").clicked() {
                                        console.stop();
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new("○ stopped").weak().small(),
                                    );
                                    if ui
                                        .button("Start Runtime")
                                        .on_hover_text(
                                            "Launch the embedded organic-math-mind-runtime \
                                             as a child process — no separate terminal. \
                                             Requires a plugin bundled with --with-llm.",
                                        )
                                        .clicked()
                                    {
                                        do_start = true;
                                    }
                                }
                                if ui.button("Clear").clicked() {
                                    console.clear_log();
                                }
                            });
                            // Live log (stdout+stderr of the child), auto-scrolled.
                            egui::ScrollArea::vertical()
                                .id_source("mind_console_log")
                                .max_height(160.0)
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for line in console.lines() {
                                        ui.label(
                                            egui::RichText::new(line).monospace().small(),
                                        );
                                    }
                                });
                            // Command REPL input (Enter or Send → child stdin).
                            ui.horizontal(|ui| {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut state.mind_cmd_input)
                                        .hint_text("gen · load · temp 0.7 · status · stop · help")
                                        .desired_width(f32::INFINITY),
                                );
                                let enter = resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                if (ui.button("Send").clicked() || enter)
                                    && !state.mind_cmd_input.trim().is_empty()
                                {
                                    console.send_command(&state.mind_cmd_input);
                                    state.mind_cmd_input.clear();
                                    resp.request_focus();
                                }
                            });
                        }
                        if do_start {
                            match mind_runtime_path() {
                                Some(exe) => crate::mind_console::MindConsole::start(
                                    &mind_console,
                                    &exe,
                                ),
                                None => {
                                    if let Ok(mut c) = mind_console.lock() {
                                        c.note(
                                            "mind-console: organic-math-mind-runtime not \
                                             found — rebuild/deploy with --with-llm to \
                                             embed it in the bundle.",
                                        );
                                    }
                                }
                            }
                        }
                    });

                    card(&mut c[2], "Chat / Agent", |ui| {
                        ui.label(
                            egui::RichText::new(
                                "Talk to the Performer — it plays Organon by setting \
                                 parameters. Moving a slider releases that param's \
                                 agent hold (last-touched-wins).",
                            )
                            .weak()
                            .small(),
                        );
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for line in &state.chat_log {
                                    ui.label(line);
                                }
                            });
                        ui.separator();
                        ui.add(
                            egui::TextEdit::multiline(&mut state.chat_input)
                                .desired_rows(2)
                                .hint_text("Ask the Performer…")
                                .desired_width(f32::INFINITY),
                        );
                        if ui.button("Send").clicked()
                            && !state.chat_input.trim().is_empty()
                        {
                            let msg = state.chat_input.trim().to_string();
                            write_chat_sidecar(&msg, &chat_gen);
                            agent_on.store(true, Ordering::Relaxed);
                            crate::mind_log::append(
                                crate::mind_log::MindEvent::Prompt,
                                "editor",
                                &msg,
                            );
                            state.chat_log.push(format!("you: {msg}"));
                            state.chat_input.clear();
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new(
                                "— local model (OpenAI-compatible: Ollama / LM Studio / \
                                 llama.cpp / MLX) —",
                            )
                            .weak()
                            .small(),
                        );
                        ui.horizontal(|ui| {
                            ui.label("endpoint");
                            ui.text_edit_singleline(&mut state.agent_endpoint);
                        });
                        ui.horizontal(|ui| {
                            ui.label("model");
                            ui.text_edit_singleline(&mut state.agent_model);
                        });
                        if ui.button("Save endpoint / model").clicked() {
                            crate::agent::AgentConfig {
                                endpoint: state.agent_endpoint.clone(),
                                model: state.agent_model.clone(),
                            }
                            .save();
                        }
                        ui.separator();
                        // #425: name saved presets with the local model. Keys off
                        // the endpoint above being reachable — NOT on the chat being
                        // engaged — so it works even if you never send a message.
                        ui.checkbox(
                            &mut state.auto_name_presets,
                            "✨ Auto-name presets on save",
                        )
                        .on_hover_text(
                            "When saving a preset, ask the local model for a name \
                             based on the generator, surface, material and look — \
                             instead of \"Preset N\". No reachable model → the \
                             provisional name stands. You can always rename.",
                        );
                        ui.separator();
                        if ui
                            .button("Release agent")
                            .on_hover_text(
                                "Clear all agent parameter holds (the sliders + CC \
                                 otherwise win back per-param on touch).",
                            )
                            .clicked()
                        {
                            release_gen.fetch_add(1, Ordering::Relaxed);
                            state.chat_log.push("— released all agent holds —".into());
                        }
                        ui.separator();
                        ui.label(
                            egui::RichText::new(
                                "— phrase plan (debug executor, no model) —",
                            )
                            .weak()
                            .small(),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .button("Load plan (JSON)…")
                                .on_hover_text(
                                    "Pick a phrase-plan JSON; its contents are \
                                     written to organic-math-plan.txt and the \
                                     visual's debug executor applies it.",
                                )
                                .clicked()
                            {
                                pick_plan_async(plan_gen.clone());
                                state.chat_log.push("— loading phrase plan… —".into());
                            }
                            if ui
                                .button("Reload plan")
                                .on_hover_text(
                                    "Re-read organic-math-plan.txt and re-apply it \
                                     (bumps plan_gen).",
                                )
                                .clicked()
                            {
                                plan_gen.fetch_add(1, Ordering::Relaxed);
                                state.chat_log.push("— reloaded phrase plan —".into());
                            }
                        });
                        // Held-params readout: the visual (which runs the agent)
                        // publishes it to the status sidecar.
                        let status = std::fs::read_to_string(ipc::agent_status_path())
                            .unwrap_or_default();
                        let mut lines = status.lines();
                        let held = lines.next().unwrap_or("").trim();
                        let reply = lines.next().unwrap_or("").trim();
                        ui.label(
                            egui::RichText::new(format!(
                                "holding: {}",
                                if held.is_empty() { "(none)" } else { held }
                            ))
                            .small(),
                        );
                        if !reply.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("agent: {reply}")).small(),
                            );
                        }
                    });

                    // #423 Tier 1 — The atlas: the model library as a
                    // resource-aware design space, from headers alone.
                    card(&mut c[2], "Design Space (atlas)", |ui| {
                        // Seed the card's defaults once (PresetUi derives Default → 0s).
                        if !state.atlas_seeded {
                            let profiles = crate::math::builtin_hardware_profiles();
                            state.atlas_profile_idx =
                                profiles.iter().position(|p| p.name == "M2 Max").unwrap_or(0);
                            state.atlas_ctx_tokens = 4096;
                            state.atlas_seeded = true;
                        }
                        // Adopt a profile the "Load…" thread just parsed (once).
                        if let Some(p) =
                            atlas_loaded_profile.lock().ok().and_then(|mut g| g.take())
                        {
                            state.atlas_custom_profile = Some(p);
                        }
                        ui.label(egui::RichText::new(
                            "Scan the models you already have; they appear as a \
                             constellation in a roofline field — which are \
                             memory-bound, which would saturate compute, how the \
                             quant families ladder — before any is ever run. From \
                             GGUF headers only (no inference). Tags: = measured · \
                             ~ derived · ? proxy.",
                        ).weak().small());

                        // Hardware profile — the roofline ceilings.
                        let profiles = crate::math::builtin_hardware_profiles();
                        let cur_name = if let Some(p) = &state.atlas_custom_profile {
                            format!("{} (JSON)", p.name)
                        } else {
                            profiles
                                .get(state.atlas_profile_idx)
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "M2 Max".into())
                        };
                        egui::ComboBox::from_label("hardware")
                            .selected_text(cur_name)
                            .show_ui(ui, |ui| {
                                for (i, p) in profiles.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            state.atlas_custom_profile.is_none()
                                                && state.atlas_profile_idx == i,
                                            &p.name,
                                        )
                                        .clicked()
                                    {
                                        state.atlas_profile_idx = i;
                                        state.atlas_custom_profile = None;
                                    }
                                }
                            });
                        if ui
                            .button("Load Hardware Profile (JSON)…")
                            .on_hover_text(
                                "A JSON { \"name\", \"bandwidth_gbps\", \
                                 \"peak_gflops\" } overriding the built-in ceilings \
                                 for your exact machine.",
                            )
                            .clicked()
                        {
                            pick_hw_profile_async(
                                atlas_readout.clone(),
                                atlas_loaded_profile.clone(),
                            );
                        }

                        // Context length — the traffic is workload-specific.
                        let mut ctx = state.atlas_ctx_tokens as f32;
                        if ui
                            .add(
                                egui::Slider::new(&mut ctx, 512.0..=32768.0)
                                    .text("context")
                                    .logarithmic(true),
                            )
                            .on_hover_text(
                                "The context length the per-token traffic (and so \
                                 each model's roofline position) is stated at. \
                                 Positions are workload-specific — there is no \
                                 single 'best model'.",
                            )
                            .changed()
                        {
                            state.atlas_ctx_tokens = ctx.round().max(1.0) as u32;
                        }

                        // Scan — walks a directory, header-parses each .gguf.
                        if ui
                            .button("Scan Model Library…")
                            .on_hover_text(
                                "Pick a folder of .gguf models (e.g. \
                                 ~/.lmstudio/models/…). Each header is parsed (cheap \
                                 — metadata + tensor directory, no weights) and \
                                 placed in the design space. Then select the Neural \
                                 Network generator + topology = 'Connectome (loaded)' \
                                 to fly the constellation.",
                            )
                            .clicked()
                        {
                            let profile = state
                                .atlas_custom_profile
                                .clone()
                                .unwrap_or_else(|| {
                                    profiles
                                        .get(state.atlas_profile_idx)
                                        .cloned()
                                        .unwrap_or_default()
                                });
                            scan_library_async(
                                atlas_gen.clone(),
                                atlas_on.clone(),
                                atlas_readout.clone(),
                                profile,
                                state.atlas_ctx_tokens,
                            );
                        }

                        let readout = atlas_readout.lock().map(|s| s.clone()).unwrap_or_default();
                        if readout.is_empty() {
                            ui.label(egui::RichText::new("No library scanned.").weak().small());
                        } else {
                            ui.label(egui::RichText::new(readout).monospace().small());
                        }

                        ui.separator();
                        let mut on = atlas_on.load(Ordering::Relaxed) != 0;
                        if ui
                            .checkbox(&mut on, "Atlas active")
                            .on_hover_text(
                                "Show the atlas views: the roofline inset and \
                                 the design-space axis labels. The constellation \
                                 itself is fed to the Neural Network generator by \
                                 the scan — select it with topology = Connectome \
                                 to see it.",
                            )
                            .changed()
                        {
                            atlas_on.store(on as u32, Ordering::Relaxed);
                        }
                        let mut rf = atlas_roofline.load(Ordering::Relaxed) != 0;
                        if ui
                            .checkbox(&mut rf, "Roofline inset")
                            .on_hover_text(
                                "Draw the log-log roofline plot (operational \
                                 intensity vs attainable tok/s) as a screen inset, \
                                 each model a dot on the ceiling.",
                            )
                            .changed()
                        {
                            atlas_roofline.store(rf as u32, Ordering::Relaxed);
                        }
                    });
                });
            }

            // ── Synth tab (#354 — the Duo-Field synth, moved off Generator) ──
            if state.tab == preset::UiTab::Synth {
            fixed_columns(ui, |c| {
                let w0 = (c[0].available_width() - COL_PAD).max(150.0);
                // The synth sonifies the Maxwell / Acoustic field; on other
                // generators there's no field to hear. Off = silent + a
                // byte-identical passthrough.
                if gmode == GeneratorMode::MaxwellField || gmode == GeneratorMode::Acoustic {
                card(&mut c[0], "Sound (Duo-Field synth)", |ui| {
                    crow(ui, "synth on", &params.sn_on, setter);
                    param_combo(ui, w0, "engine", &params.sn_mode, setter);
                    param_combo(ui, w0, "play mode", &params.sn_play_mode, setter);
                    srow(ui, w0, "gain", &params.sn_gain, setter);
                    srow(ui, w0, "wet", &params.sn_mix, setter);
                    ui.label(egui::RichText::new("— generative bed (follows Acoustic / Maxwell; Cavity → its standing wave) —").weak().small());
                    srow(ui, w0, "k → pitch (Hz/k)", &params.sn_tuning, setter);
                    srow(ui, w0, "bed amp", &params.sn_gen_amp, setter);
                    ui.label(egui::RichText::new("— oscillator lattice (engine = Lattice) —").weak().small());
                    param_combo(ui, w0, "tuning", &params.sn_tuning_layout, setter);
                    srow(ui, w0, "bank size", &params.sn_bank, setter);
                    srow(ui, w0, "spread", &params.sn_tune_spread, setter);
                    srow(ui, w0, "stretch", &params.sn_tune_stretch, setter);
                    srow(ui, w0, "shell radius", &params.sn_shell_r, setter);
                    srow(ui, w0, "breathe (Hz)", &params.sn_shell_rate, setter);
                    ui.label(egui::RichText::new("— struck cavities (engine = Modal) —").weak().small());
                    srow(ui, w0, "decay T60 (s)", &params.sn_t60, setter);
                    srow(ui, w0, "brightness", &params.sn_bright, setter);
                    ui.label(egui::RichText::new("— granular aura (engine = Granular) —").weak().small());
                    srow(ui, w0, "grain size (s)", &params.sn_grain_size, setter);
                    srow(ui, w0, "grain density", &params.sn_grain_density, setter);
                    ui.label(egui::RichText::new("— scanned wavetable (engine = Wavetable) —").weak().small());
                    ui.label(egui::RichText::new("scans the shell cross-section; play a note to pitch it").weak().small());
                    ui.label(egui::RichText::new("— voices (instrument) —").weak().small());
                    srow(ui, w0, "attack", &params.sn_attack, setter);
                    srow(ui, w0, "decay", &params.sn_decay, setter);
                    srow(ui, w0, "sustain", &params.sn_sustain, setter);
                    srow(ui, w0, "release", &params.sn_release, setter);
                    srow(ui, w0, "glide", &params.sn_glide, setter);
                    srow(ui, w0, "bend range", &params.sn_bend_range, setter);
                    srow(ui, w0, "keyboard spread", &params.sn_place_spread, setter);
                    srow(ui, w0, "concert A", &params.sn_a4, setter);
                    ui.label(egui::RichText::new("— listener probes (L / R) —").weak().small());
                    srow(ui, w0, "L x", &params.sn_probe_lx, setter);
                    srow(ui, w0, "L y", &params.sn_probe_ly, setter);
                    srow(ui, w0, "L z", &params.sn_probe_lz, setter);
                    srow(ui, w0, "R x", &params.sn_probe_rx, setter);
                    srow(ui, w0, "R y", &params.sn_probe_ry, setter);
                    srow(ui, w0, "R z", &params.sn_probe_rz, setter);
                    crow(ui, "probe 0 rides camera", &params.sn_probe_cam, setter);
                    ui.label(egui::RichText::new("— visual time lens —").weak().small());
                    param_combo(ui, w0, "quantize", &params.sn_vis_quantize, setter);
                    srow(ui, w0, "pivot (Hz)", &params.sn_vis_pivot, setter);
                    srow(ui, w0, "visual rate", &params.sn_vis_anchor, setter);
                    srow(ui, w0, "time slope", &params.sn_vis_slope, setter);
                    srow(ui, w0, "space k", &params.sn_vis_k_anchor, setter);
                    srow(ui, w0, "space slope", &params.sn_vis_k_slope, setter);
                    help(ui, "#339 Tier 1 — put virtual microphones in the field and HEAR the \
                             wave. Two listener probes (L/R) sample the acoustic pressure the \
                             generator radiates; their spacing gives real interaural delay \
                             (stereo from physics, not a pan pot), 1/r gives distance falloff, \
                             and source separation gives beating/interference you can drag a \
                             probe through. Play mode: Generative = the bed drones on its own \
                             (no MIDI); Instrument = each held note is a radiating source you \
                             play (chords interfere in the field itself); Duet = both. The \
                             visual time lens renders the picture at an octave-offset of the \
                             sound (a fast note would strobe), preserving pitch order while \
                             compressing the span — with a hard photosensitivity ceiling. Off \
                             → silent + byte-identical passthrough. A soft limiter closes the \
                             bus so no near-field spike can slam the master.");
                });
                } else {
                card(&mut c[0], "Sound (Duo-Field synth)", |ui| {
                    ui.label(egui::RichText::new(
                        "The synth sonifies the Maxwell / Acoustic field. Select the \
                         Maxwell or Acoustic generator to play it.",
                    ).weak());
                });
                }
            });
            }

            // ── Mind tab (#317 / #367 — the visible mind) ──────────
            // Shared surface: #317 (Performer) owns col 0 "Chat / Agent";
            // #367 (Specimen) owns col 1 "Model / Specimen". This branch
            // (B1) fills only col 1; the integrator concatenates the two.

            // ── Mind tab: #482 Tier 1 — Live Telemetry dashboard ───
            // The Audio tab feels alive while sound plays; this makes the
            // Mind tab feel alive while a model infers. Editor-side egui
            // paints (mirroring AudioViz), driven purely by the existing
            // MindFrame in the activation ring — no writer/ring/`Shared`
            // change. The editor opens its OWN `MindRingReader` (mmap reads
            // are non-destructive) and repaints continuously while streaming.
            // #532 Tier 1: in Organon Mind this dashboard normally lives in the
            // always-visible bottom dock (drawn above, before the central
            // panel), so rendering it here too would show it twice. It still
            // renders inline in two cases: full Organon, which has no dock to
            // put it in, and a Mind window too small for the dock to be worth
            // drawing — losing the telemetry entirely would be worse than
            // making it scroll. `mind_observe` is pumped exactly once per
            // frame either way; double-observing would advance the smoothers
            // twice on one `dt`.
            if state.tab == preset::UiTab::Mind {
                let is_mind = crate::edition::EDITION.is_mind();
                if !is_mind {
                    mind_observe(ui.ctx(), state);
                }
                if !is_mind || !mind_bottom_dock_shown {
                    ui.separator();
                    mind_dashboard_ui(ui, state);
                }
            }

            // ── Generator tab ──────────────────────────────────────
            if state.tab == preset::UiTab::Generator {
            fixed_columns(ui, |c| {
                let w0 = (c[0].available_width() - COL_PAD).max(150.0);
                card(&mut c[0], "Generator", |ui| {
                    param_combo_sized(ui, w0, "algorithm", &params.generator, setter, 2.0 * COMBO_W);
                    help(ui, "Which algorithm builds the node field. The cards below are \
                             the active generator's parameters; surface mode, materials, \
                             lighting and look apply to every generator.");
                    // Origin mode is a property of the Original cube-field only (the
                    // strand generators build their own geometry and ignore it), so it
                    // lives with the Generator, not the shared Surface card.
                    if original {
                        param_combo_sized(ui, w0, "origin", &params.origin_mode, setter, 2.0 * COMBO_W);
                        help(ui, "Corner: the grid's corner sits at the world origin and each \
                                 arm/sheet pivots off it (the original look). Centered: the grid \
                                 is symmetric about the origin — each arm/sheet pivots off its \
                                 own centre.");
                    }
                });
                if original {
                card(&mut c[0], "Loop Geometry", |ui| {
                    srow(ui, w0, "count x", &params.loop_count_x, setter);
                    srow(ui, w0, "count y", &params.loop_count_y, setter);
                    srow(ui, w0, "count z", &params.loop_count_z, setter);
                    srow(ui, w0, "count q", &params.loop_count_q, setter);
                });
                card(&mut c[0], "Rotation", |ui| {
                    param_combo_sized(ui, w0, "func", &params.rot_func, setter, 2.0 * COMBO_W);
                    srow(ui, w0, "amp x", &params.rot_amp_x, setter);
                    srow(ui, w0, "amp y", &params.rot_amp_y, setter);
                    srow(ui, w0, "amp z", &params.rot_amp_z, setter);
                    srow(ui, w0, "speed x", &params.rot_mod_x, setter);
                    srow(ui, w0, "speed y", &params.rot_mod_y, setter);
                    srow(ui, w0, "speed z", &params.rot_mod_z, setter);
                    srow(ui, w0, "continuous", &params.continuous, setter);
                    srow(ui, w0, "wave depth", &params.cont_shape, setter);
                    help(ui, "Wave depth (continuous only): the rotation func shapes the \
                             winding speed. 0 = constant spin; up = sine breathes, \
                             triangle ramps, square gear-shifts, saw revs.");
                });
                card(&mut c[0], "Translation", |ui| {
                    param_combo_sized(ui, w0, "func", &params.trans_func, setter, 2.0 * COMBO_W);
                    srow(ui, w0, "amp x", &params.trans_amp_x, setter);
                    srow(ui, w0, "amp y", &params.trans_amp_y, setter);
                    srow(ui, w0, "amp z", &params.trans_amp_z, setter);
                    srow(ui, w0, "mod x", &params.trans_mod_x, setter);
                    srow(ui, w0, "mod y", &params.trans_mod_y, setter);
                    srow(ui, w0, "mod z", &params.trans_mod_z, setter);
                });
                card(&mut c[0], "Scaling", |ui| {
                    param_combo_sized(ui, w0, "func", &params.scale_func, setter, 2.0 * COMBO_W);
                    srow(ui, w0, "amp", &params.scale_amp, setter);
                });
                } // end generator-specific cards (Original)

                if gmode == GeneratorMode::Frenet {
                    card(&mut c[0], "Frenet Curve (κ / τ)", |ui| {
                        param_combo_sized(ui, w0, "func", &params.frenet_func, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "curvature κ", &params.frenet_kappa, setter);
                        srow(ui, w0, "κ amp", &params.frenet_kappa_amp, setter);
                        srow(ui, w0, "κ freq", &params.frenet_kappa_freq, setter);
                        srow(ui, w0, "torsion τ", &params.frenet_tau, setter);
                        srow(ui, w0, "τ amp", &params.frenet_tau_amp, setter);
                        srow(ui, w0, "τ freq", &params.frenet_tau_freq, setter);
                        help(ui, "Integrate a moving frame from curvature κ and torsion τ. \
                                 Constant κ + τ = a helix; raise the amps (modulated by \
                                 func) to make it wind and unwind. The κ/τ phase is the \
                                 global Speed clock, so it animates + rides the beat.");
                    });
                    card(&mut c[0], "Frenet Bundle", |ui| {
                        srow(ui, w0, "strands", &params.frenet_strands, setter);
                        srow(ui, w0, "nodes", &params.frenet_nodes, setter);
                        srow(ui, w0, "step (ds)", &params.frenet_step, setter);
                        srow(ui, w0, "spread", &params.frenet_spread, setter);
                        srow(ui, w0, "thickness", &params.frenet_thickness, setter);
                        help(ui, "A phase-offset bundle of curves (Grid topology) — works \
                                 with every surface mode. Membrane currently renders the \
                                 strands as swept tubes (full lofting is a follow-up).");
                    });
                }

                if gmode == GeneratorMode::Dna {
                    card(&mut c[0], "DNA Helix", |ui| {
                        param_combo_sized(ui, w0, "form", &params.dna_form, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "base pairs", &params.dna_bp, setter);
                        srow(ui, w0, "supercoil σ", &params.dna_sigma, setter);
                        srow(ui, w0, "superhelix radius", &params.dna_super_radius, setter);
                        srow(ui, w0, "twist breathe", &params.dna_twist_breathe, setter);
                        srow(ui, w0, "thickness", &params.dna_thickness, setter);
                        srow(ui, w0, "sequence seed", &params.dna_seed, setter);
                        help(ui, "Two backbones + base-pair rungs (A–T cool, G–C warm). \
                                 Supercoil σ trades twist for writhe (L = T + W): |σ| up \
                                 coils the spine into a superhelix. Twist breathe animates \
                                 that trade off the global Speed. Best in Swept Tubes.");
                    });
                    card(&mut c[0], "DNA Custom (form = Custom)", |ui| {
                        srow(ui, w0, "bp / turn", &params.dna_bp_per_turn, setter);
                        srow(ui, w0, "rise (Å)", &params.dna_rise, setter);
                        srow(ui, w0, "radius (Å)", &params.dna_radius, setter);
                        srow(ui, w0, "groove Δ (°)", &params.dna_groove, setter);
                        crow(ui, "left-handed", &params.dna_left, setter);
                        help(ui, "Only used when form = Custom (A/B/Z preset these). \
                                 Groove Δ ≠ 180° is what creates the major/minor grooves.");
                    });
                }

                if gmode == GeneratorMode::Attractor {
                    card(&mut c[0], "Strange Attractor", |ui| {
                        param_combo_sized(ui, w0, "field", &params.attr_field, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "seeds", &params.attr_seeds, setter);
                        srow(ui, w0, "seed value", &params.attr_seed, setter);
                        srow(ui, w0, "spread", &params.attr_spread, setter);
                        srow(ui, w0, "trail", &params.attr_trail, setter);
                        srow(ui, w0, "head speed", &params.attr_speed, setter);
                        srow(ui, w0, "step ×dt", &params.attr_dt, setter);
                        srow(ui, w0, "scale", &params.attr_scale, setter);
                        srow(ui, w0, "thickness", &params.attr_thickness, setter);
                        help(ui, "Forward-integrate a chaotic field (Lorenz, Aizawa, …) \
                                 from N seeds → flowing streamlines. Head speed (× global \
                                 Speed) slides the trail along each trajectory, so it \
                                 flows + rides the beat. Best in Swept Tubes / Metaball; \
                                 Membrane falls back to tubes (Streamlines topology).");
                    });
                }

                if gmode == GeneratorMode::Harmonic {
                    card(&mut c[0], "Spherical-Harmonic Modes", |ui| {
                        srow(ui, w0, "mode 0", &params.harm_mode0, setter);
                        srow(ui, w0, "amp 0", &params.harm_amp0, setter);
                        srow(ui, w0, "freq 0", &params.harm_freq0, setter);
                        srow(ui, w0, "mode 1", &params.harm_mode1, setter);
                        srow(ui, w0, "amp 1", &params.harm_amp1, setter);
                        srow(ui, w0, "freq 1", &params.harm_freq1, setter);
                        srow(ui, w0, "mode 2", &params.harm_mode2, setter);
                        srow(ui, w0, "amp 2", &params.harm_amp2, setter);
                        srow(ui, w0, "freq 2", &params.harm_freq2, setter);
                        help(ui, "A sphere displaced by Σ ampₖ·cos(freqₖ·Speed)·Yₗᵐ — a \
                                 pulsing bell. mode index → Yₗᵐ: 0=Y₀₀, 1=Y₁₀, 4=Y₂₀, \
                                 6=Y₂₂, 8=Y₃₀, 11=Y₃₃, 12=Y₄₀, 14=Y₄₄. freq pulses it off \
                                 the global Speed clock. Best in Metaball (smooth bell).");
                    });
                    card(&mut c[0], "Spherical-Harmonic Grid", |ui| {
                        srow(ui, w0, "radius", &params.harm_radius, setter);
                        srow(ui, w0, "θ resolution", &params.harm_theta, setter);
                        srow(ui, w0, "φ resolution", &params.harm_phi, setter);
                        srow(ui, w0, "thickness", &params.harm_thickness, setter);
                    });
                    card(&mut c[0], "Soft-body Bell (physical)", |ui| {
                        srow(ui, w0, "physical", &params.bell_physical, setter);
                        srow(ui, w0, "stroke depth", &params.bell_stroke_depth, setter);
                        srow(ui, w0, "openness", &params.bell_open, setter);
                        srow(ui, w0, "stiffness", &params.bell_stiffness, setter);
                        srow(ui, w0, "damping", &params.bell_damping, setter);
                        srow(ui, w0, "stroke rate", &params.bell_speed, setter);
                        help(ui, "Physical turns the bell into a real XPBD soft body: it \
                                 genuinely contracts + recoils (volume-preserving) instead \
                                 of replaying the harmonic sum. The contraction pulse is \
                                 beat-paced — stroke rate sets pulses per bar (lower = \
                                 slower, flowier); stroke depth how hard it squeezes; \
                                 openness the flare; stiffness/damping the softness (lower \
                                 stiffness = floppier, higher damping = more elastic glide). \
                                 Reuses the radius / θ-φ resolution / thickness above. Best \
                                 in Membrane (the bell sheet) or Metaball. Fluid jet \
                                 propulsion is the next step.");
                    });
                }

                if gmode == GeneratorMode::LSystem {
                    card(&mut c[0], "L-system (plant)", |ui| {
                        param_combo_sized(ui, w0, "system", &params.ls_system, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "depth", &params.ls_depth, setter);
                        srow(ui, w0, "angle", &params.ls_angle, setter);
                        srow(ui, w0, "step", &params.ls_step, setter);
                        srow(ui, w0, "growth", &params.ls_grow, setter);
                        srow(ui, w0, "sway amp", &params.ls_sway_amp, setter);
                        srow(ui, w0, "sway freq", &params.ls_sway_freq, setter);
                        srow(ui, w0, "thickness", &params.ls_thickness, setter);
                        help(ui, "Rewrite a plant grammar and walk it with a branching \
                                 turtle (Fern/Bush/Tree/Seaweed). Depth grows detail \
                                 (node count explodes — capped at 7). Growth unfurls it \
                                 from the base; sway animates the turn off the global \
                                 Speed. Best in Swept Tubes; Membrane degrades to tubes.");
                    });
                }

                if gmode == GeneratorMode::CurlNoise {
                    card(&mut c[0], "Curl-noise Flow", |ui| {
                        srow(ui, w0, "seeds", &params.cn_seeds, setter);
                        srow(ui, w0, "seed value", &params.cn_seed, setter);
                        srow(ui, w0, "spread", &params.cn_spread, setter);
                        srow(ui, w0, "field scale", &params.cn_scale, setter);
                        srow(ui, w0, "steps", &params.cn_steps, setter);
                        srow(ui, w0, "step dt", &params.cn_dt, setter);
                        srow(ui, w0, "flow speed", &params.cn_flow, setter);
                        srow(ui, w0, "containment", &params.cn_bound, setter);
                        srow(ui, w0, "thickness", &params.cn_thickness, setter);
                        help(ui, "Particles advected through the curl of a noise field \
                                 (divergence-free) → smooth ink/smoke streamlines. Flow \
                                 speed evolves the field off the global Speed clock; \
                                 field scale = turbulence; containment pulls toward the \
                                 centre. Best in Swept Tubes / Metaball.");
                    });
                }

                if gmode == GeneratorMode::Polarization {
                    card(&mut c[0], "Circular Polarization", |ui| {
                        srow(ui, w0, "rings (θ)", &params.pol_rings, setter);
                        srow(ui, w0, "spokes (φ)", &params.pol_spokes, setter);
                        srow(ui, w0, "spread °", &params.pol_spread, setter);
                        srow(ui, w0, "samples/ray", &params.pol_samples, setter);
                        srow(ui, w0, "ray length", &params.pol_len, setter);
                        srow(ui, w0, "wavenumber k", &params.pol_k, setter);
                        srow(ui, w0, "amplitude", &params.pol_amp, setter);
                        srow(ui, w0, "falloff (1/r)", &params.pol_falloff, setter);
                        srow(ui, w0, "swirl", &params.pol_swirl, setter);
                        srow(ui, w0, "thickness", &params.pol_thickness, setter);
                        crow(ui, "left-handed", &params.pol_handed, setter);
                        crow(ui, "show B helix", &params.pol_show_b, setter);
                        help(ui, "The rotating E-field of a circularly polarized wave, \
                                 E ∝ (1/r)[cos·ê₁ + sin·ê₂], traced along a (θ,φ) fan of \
                                 rays from a point source. 1 spoke = a lone corkscrew; a \
                                 dense fan = the radiating eye. k = helix tightness, \
                                 spread = single axis→full sphere, swirl precesses the fan \
                                 off the global Speed. Show B adds the perpendicular helix \
                                 (warm E / cool B). Grid topology: Swept Tubes = glassy \
                                 filaments, Metaball = a plasma core, Membrane = a rippling \
                                 shell across the fan.");
                    });
                }

                if gmode == GeneratorMode::MaxwellField {
                    card(&mut c[0], "Maxwell Field", |ui| {
                        crow(ui, "field lines (else lattice)", &params.mx_lines, setter);
                        crow(ui, "dipoles (else charges)", &params.mx_dipoles, setter);
                        srow(ui, w0, "generator E↔B", &params.mx_gen_blend, setter);
                        srow(ui, w0, "sources", &params.mx_sources, setter);
                        srow(ui, w0, "separation", &params.mx_separation, setter);
                        srow(ui, w0, "phase offset", &params.mx_phase, setter);
                        srow(ui, w0, "swirl", &params.mx_swirl, setter);
                        srow(ui, w0, "near-field", &params.mx_near, setter);
                        srow(ui, w0, "wavenumber k", &params.mx_k, setter);
                        srow(ui, w0, "amplitude", &params.mx_amp, setter);
                        srow(ui, w0, "source clamp", &params.mx_rmin, setter);
                        srow(ui, w0, "thickness", &params.mx_thickness, setter);
                        ui.label(egui::RichText::new("— oscillation —").weak().small());
                        crow(ui, "tempo sync (else Speed clock)", &params.mx_osc_sync, setter);
                        param_combo(ui, w0, "osc division", &params.mx_osc_div, setter);
                        srow(ui, w0, "E↔B phase ° (0 far ↔ 90 near)", &params.mx_eb_phase, setter);
                        ui.label(egui::RichText::new("— lattice —").weak().small());
                        srow(ui, w0, "rings (θ)", &params.mx_rings, setter);
                        srow(ui, w0, "spokes (φ)", &params.mx_spokes, setter);
                        srow(ui, w0, "samples/ray", &params.mx_samples, setter);
                        srow(ui, w0, "ray length", &params.mx_raylen, setter);
                        srow(ui, w0, "spread °", &params.mx_spread, setter);
                        crow(ui, "unit-field spokes (else waves)", &params.mx_norm_field, setter);
                        ui.label(egui::RichText::new("— field lines —").weak().small());
                        srow(ui, w0, "seeds/source", &params.mx_seeds, setter);
                        srow(ui, w0, "line steps", &params.mx_steps, setter);
                        srow(ui, w0, "step ds", &params.mx_ds, setter);
                        srow(ui, w0, "line bound", &params.mx_bound, setter);
                        ui.label(egui::RichText::new("— energize (#247) —").weak().small());
                        crow(ui, "energize aura by |E|²", &params.mn_energize, setter);
                        srow(ui, w0, "energy gain", &params.mn_gain, setter);
                        srow(ui, w0, "energy knee", &params.mn_knee, setter);
                        srow(ui, w0, "energy hue", &params.mn_hue, setter);
                        crow(ui, "finite antenna (rod)", &params.mn_antenna, setter);
                        srow(ui, w0, "antenna length", &params.mn_antenna_len, setter);
                        srow(ui, w0, "energy → dye + liquid", &params.mn_dye_inject, setter);
                        srow(ui, w0, "aura E↔B", &params.mx_aura_blend, setter);
                        crow(ui, "force drive (stir by the field)", &params.mn_force, setter);
                        srow(ui, w0, "force strength", &params.mn_force_gain, setter);
                        srow(ui, w0, "stir rate (Hz, fluid swirl)", &params.mn_stir_rate, setter);
                        srow(ui, w0, "acoustic pump (beat)", &params.mn_pump, setter);
                        srow(ui, w0, "beat spin force", &params.mn_swirl_beat, setter);
                        srow(ui, w0, "spin slowdown", &params.mn_swirl_decay, setter);
                        srow(ui, w0, "beat mode (−turbine ↔ +dynamo)", &params.mn_mode_mix, setter);
                        srow(ui, w0, "ring frequency (Hz)", &params.mn_ring_freq, setter);
                        srow(ui, w0, "hue cycle (beat)", &params.mn_hue_cycle, setter);
                        srow(ui, w0, "pump size", &params.mn_pump_scale, setter);
                        srow(ui, w0, "energy contrast (core)", &params.mn_energy_contrast, setter);
                        ui.label(egui::RichText::new("— audio drive (#248) —").weak().small());
                        crow(ui, "audio drives the dipole", &params.ad_drive, setter);
                        srow(ui, w0, "drive amount", &params.ad_amount, setter);
                        srow(ui, w0, "drive floor", &params.ad_floor, setter);
                        crow(ui, "spectrum → multipole", &params.ad_multipole, setter);
                        srow(ui, w0, "band wavelength spread", &params.ad_spread, setter);
                        srow(ui, w0, "colour by band", &params.ad_band_hue, setter);
                        srow(ui, w0, "stereo lean", &params.ad_stereo, setter);
                        srow(ui, w0, "pitch → rate", &params.ad_pitch, setter);
                        srow(ui, w0, "waveform shells", &params.ad_wave, setter);
                        help(ui, "The real E/B fields of point charges / oscillating dipoles \
                                 (superposed, retarded time) — self-consistent, unlike Circular \
                                 Polarization. One dipole = the radiation lobe (Membrane lofts \
                                 the shell); switch to field lines + 2 charges for the dipole \
                                 rose. Near-field adds the 1/r³ structure; k = retarded lag; \
                                 swirl orbits the sources off the global Speed. Best in Swept \
                                 Tubes / Metaball + bloom/HDR.\n\nEnergize: turn on the Particle \
                                 Aura (Lite tier) and this lights each mote by the field's real \
                                 energy density ½(|E|²+|B|²) — the fluorescent-tube demo. Motes \
                                 still drift along the field lines but now glow by magnitude: \
                                 bright in the strong zones, dark in the nulls, obeying the 1/r \
                                 near-field falloff. gain = brightness, knee = HDR ceiling, \
                                 hue = ember colour.\n\nFinite antenna (Tier 2): model the source \
                                 as a driven ROD on Z carrying the standing-wave current \
                                 I(z)=I₀·sin(k(L/2−|z|)) instead of the idealized point dipole — \
                                 the bound charge piles up at the tips, so the cloud lights \
                                 BRIGHT-ENDS / DIM-CENTRE (the literal fluorescent-tube demo). \
                                 kL/2=π/2 is a half-wave rod; longer L adds standing-wave nodes.\
                                 \n\nAudio drive (#248): a speaker IS an acoustic dipole — this \
                                 drives ours from the live music. The loudness envelope (smoothed \
                                 RMS; needs Audio Reactive on, Motion tab) scales the source's \
                                 drive amplitude, so the energy cloud brightens and swells with \
                                 the track's dynamics (E,B scale linearly → |E|² breathes \
                                 quadratically). floor = the dim idle field on silence. Honest: \
                                 the audio modulates the SOURCE's parameters; the field math \
                                 stays the real retarded radiation — the 20 Hz–20 kHz carrier \
                                 itself is never rendered (a declared scaling, not a physics \
                                 claim).\n\nSpectrum → multipole (Tier 2): each FFT band drives \
                                 a distinct MULTIPOLE MOMENT — bass a big dipole lobe, highs \
                                 higher-order moments (binomial dipole arrays; the multipole \
                                 expansion IS the spherical-harmonic series) — so the field's \
                                 spatial shape encodes the spectrum: a bass note fattens the \
                                 low-order lobe, cymbals sparkle fine high-order structure. \
                                 Replaces the point/antenna source while on. Wavelength spread \
                                 compresses the honest per-band λ ∝ 1/f ratio into a watchable \
                                 range; colour-by-band tints dye + arrows from ember (bass) \
                                 across the wheel (highs).");
                    });
                    // #412 Tier 3 Phase 0: the FDTD solver — marches the curl
                    // equations on a grid so the field propagates (a pulse
                    // launches and travels at c) instead of the closed form.
                    card(&mut c[0], "FDTD Solver (#412)", |ui| {
                        crow(ui, "run solver (else closed form)", &params.fdtd_on, setter);
                        param_combo(ui, w0, "source", &params.fdtd_source, setter);
                        srow(ui, w0, "frequency ω", &params.fdtd_freq, setter);
                        srow(ui, w0, "drive", &params.fdtd_drive, setter);
                        srow(ui, w0, "resolution (cells/axis)", &params.fdtd_res, setter);
                        srow(ui, w0, "sub-steps / frame", &params.fdtd_substeps, setter);
                        srow(ui, w0, "sponge cells (0 = box)", &params.fdtd_boundary, setter);
                        srow(ui, w0, "domain extent", &params.fdtd_extent, setter);
                        help(ui, "Replaces the closed-form Maxwell field with a real-time \
                                 CPU FDTD solver: a Yee lattice marching B from −∇×E then \
                                 E from ∇×B, so the field is EMERGENT — a source launches a \
                                 disturbance that propagates outward at c (retardation you \
                                 watch happen) and reflects off the walls. Select the Volume \
                                 surface to see the live energy cloud. Source: Pulse = a \
                                 one-shot Gaussian wavelet (watch it launch); CW = a \
                                 continuous sinusoid (steady radiation). Sponge cells absorb \
                                 at the walls (0 = a reflecting PEC box that rings). Higher \
                                 resolution + sub-steps = sharper + faster waves but more CPU. \
                                 Off → the analytic path is byte-identical. Phase 0 of #412 \
                                 (GPU port, materials, field-lines/aura, audio-driven cavity \
                                 are the next phases).");
                    });
                }

                if gmode == GeneratorMode::Acoustic {
                    card(&mut c[0], "Acoustic Field", |ui| {
                        param_combo(ui, w0, "source", &params.ac_source, setter);
                        srow(ui, w0, "wavenumber k", &params.ac_k, setter);
                        srow(ui, w0, "circulation strength", &params.ac_near, setter);
                        srow(ui, w0, "amplitude", &params.ac_amp, setter);
                        srow(ui, w0, "separation", &params.ac_separation, setter);
                        srow(ui, w0, "source clamp", &params.ac_rmin, setter);
                        srow(ui, w0, "compress↔transverse — geometry (E↔B)", &params.ac_blend, setter);
                        crow(ui, "unit-field spokes (else waves)", &params.ac_norm_field, setter);
                        ui.label(egui::RichText::new("— oscillation (shared Duo-Field clock) —").weak().small());
                        crow(ui, "tempo sync (else Speed clock)", &params.mx_osc_sync, setter);
                        param_combo(ui, w0, "osc division", &params.mx_osc_div, setter);
                        ui.label(egui::RichText::new("— lattice —").weak().small());
                        srow(ui, w0, "rings (θ)", &params.ac_rings, setter);
                        srow(ui, w0, "spokes (φ)", &params.ac_spokes, setter);
                        srow(ui, w0, "samples/ray", &params.ac_samples, setter);
                        srow(ui, w0, "ray length", &params.ac_raylen, setter);
                        srow(ui, w0, "spread °", &params.ac_spread, setter);
                        srow(ui, w0, "thickness", &params.ac_thickness, setter);
                        ui.label(egui::RichText::new("— particle channel —").weak().small());
                        srow(ui, w0, "compress↔transverse — aura (E↔B)", &params.ac_aura_blend, setter);
                        crow(ui, "energize aura by acoustic energy", &params.mn_energize, setter);
                        srow(ui, w0, "energy gain", &params.mn_gain, setter);
                        srow(ui, w0, "energy knee", &params.mn_knee, setter);
                        srow(ui, w0, "energy hue", &params.mn_hue, setter);
                        srow(ui, w0, "intensity flux (I = p·u)", &params.ac2_intensity, setter);
                        ui.label(egui::RichText::new("— cavity / Chladni (Tier 4) —").weak().small());
                        param_combo(ui, w0, "model", &params.ac2_model, setter);
                        srow(ui, w0, "cavity nx", &params.ac2_nx, setter);
                        srow(ui, w0, "cavity ny", &params.ac2_ny, setter);
                        srow(ui, w0, "cavity nz", &params.ac2_nz, setter);
                        srow(ui, w0, "cavity beat morph", &params.ac2_morph, setter);
                        srow(ui, w0, "cavity scale", &params.ac2_cav_scale, setter);
                        srow(ui, w0, "cavity mode tween", &params.ac2_tween, setter);
                        ui.label(egui::RichText::new("— cavity 3-D audio breathe (Tier 5) —").weak().small());
                        srow(ui, w0, "audio → mode X", &params.ac2_audio_x, setter);
                        srow(ui, w0, "audio → mode Y", &params.ac2_audio_y, setter);
                        srow(ui, w0, "audio → mode Z", &params.ac2_audio_z, setter);
                        ui.label(egui::RichText::new("— audio drive (#325) —").weak().small());
                        crow(ui, "audio drives the source", &params.ad_drive, setter);
                        srow(ui, w0, "drive amount", &params.ad_amount, setter);
                        srow(ui, w0, "drive floor", &params.ad_floor, setter);
                        crow(ui, "spectrum → multipole", &params.ad_multipole, setter);
                        srow(ui, w0, "stereo lean", &params.ad_stereo, setter);
                        srow(ui, w0, "pitch → rate", &params.ad_pitch, setter);
                        srow(ui, w0, "beat pump", &params.ac_beat_pump, setter);
                        help(ui, "A radiating SOUND source rendered as a two-channel Duo-Field \
                                 (#325) — the acoustic analog of Maxwell's E/B. The two orthogonal \
                                 channels are COMPRESSION (longitudinal: the radial pressure wave, a \
                                 breathing multipole shell — Membrane lofts it) and TRANSVERSE flow \
                                 (the particle-velocity u, which carries the 3-D out-of-plane \
                                 structure — the 'extrusion' — PLUS an azimuthal circulation swirl \
                                 90° out of phase, the acoustic 'B'). Each of the geometry and the \
                                 Particle Aura has its OWN independent compress↔transverse crossfade, \
                                 exactly like Maxwell's E↔B: 0 = the radial breathing shell, 1 = the \
                                 transverse flow (extrudes + swirls), between = a helix. Default = \
                                 geometry on compression + aura on transverse, so you see both at \
                                 once. Monopole = a pulsating sphere; dipole = the figure-8 lobe with \
                                 an equatorial pressure NODE, its velocity flow extruding out of \
                                 plane; quadrupoles add finer nodal structure. 'circulation' scales \
                                 the azimuthal swirl mixed into the transverse flow. (Honest: sound \
                                 is longitudinal, so the swirl is a synthesized companion; the \
                                 velocity part is real.) \
                                 \n\nMost on-theme in an \
                                 AUDIO visualiser: the field IS sound. Audio drive (#325, shares the \
                                 #248 dipole spine): loudness → drive amplitude (the cloud breathes), \
                                 spectrum → multipole moments (bass = the fat lobe, highs = fine \
                                 structure), pitch → oscillation rate (pitch → wavelength, the most \
                                 honest mapping here), stereo → source lean, and the beat PUMPS the \
                                 source amplitude (a speaker pushing air). Needs Audio Reactive + \
                                 Pulse on (Motion tab). Best in Swept Tubes / Metaball + bloom/HDR.\
                                 \n\nTier 4 — Cavity / Chladni: switch the model to Cavity for a \
                                 bounded rectangular room mode (nx,ny,nz) instead of a radiating \
                                 source. Its pressure nodal planes are the 3-D generalisation of a \
                                 Chladni plate's sand lines — the definitive 'visible nodes' \
                                 showpiece. 'cavity beat morph' > 0 walks the modes on the beat so \
                                 the pattern reorganises musically; 'cavity scale' sets the box \
                                 size (mode wavelengths). Tier 4 — Intensity flux (the tri-field): \
                                 turn up 'intensity flux' and the Aura advects motes along the \
                                 acoustic intensity I = p·u — the direction sound energy actually \
                                 flows (outward for a radiating source) — glowing by |p·u|. The \
                                 third channel (E geometry, B particles, S = energy flux).\
                                 \n\nTier 5 — 3-D + audio: the beat mode-walk now steps ALL three \
                                 axes (nx, ny, nz) so the reorganisation is genuinely 3-D, not \
                                 flat. 'cavity mode tween' softens the walk — 0 = hard cut (the \
                                 pattern jumps on the beat), up = it holds then glides between \
                                 mode sets, so the nodal planes slide smoothly. 'audio → mode \
                                 X/Y/Z' independently breathe each axis: with 'audio drives the \
                                 source' on, louder music packs more nodal planes along that axis \
                                 (and the audio drive now swells the cavity amplitude, like the \
                                 radiating source). Set the beat/tempo/audio source in the \
                                 Sync / Tempo card (Settings tab).");
                    });
                }

                if gmode == GeneratorMode::MapAttractor {
                    card(&mut c[0], "Density-Map Attractor", |ui| {
                        param_combo(ui, w0, "map", &params.ma_kind, setter);
                        srow(ui, w0, "parameter a", &params.ma_a, setter);
                        srow(ui, w0, "parameter b", &params.ma_b, setter);
                        srow(ui, w0, "parameter c", &params.ma_c, setter);
                        srow(ui, w0, "parameter d", &params.ma_d, setter);
                        param_combo(ui, w0, "color", &params.ma_color, setter);
                        srow(ui, w0, "anim → a", &params.ma_a_drive, setter);
                        srow(ui, w0, "anim → b", &params.ma_b_drive, setter);
                        srow(ui, w0, "points (K)", &params.ma_points_k, setter);
                        srow(ui, w0, "warm-up", &params.ma_warmup, setter);
                        srow(ui, w0, "scale", &params.ma_scale, setter);
                        srow(ui, w0, "point size", &params.ma_size, setter);
                        srow(ui, w0, "intensity", &params.ma_intensity, setter);
                        help(ui, "Iterates a discrete 2-D map for many points and draws \
                                 the visited-set density as an additive glow. 'map' picks \
                                 the family: Complexus (the complex-holomorphic seed, \
                                 x' = sin(x^2 - y^2 + a), y' = cos(2xy + b)) plus the classic \
                                 strange attractors Clifford, de Jong, Pickover (fractal \
                                 dream), Gumowski-Mira and Hopalong. Clifford / de Jong / \
                                 Pickover use all four coefficients a/b/c/d; the others read \
                                 only the ones they need (c/d are inert for them). 'color' \
                                 tints each splat by local dynamics: Step Speed |d| (the \
                                 default), Iteration Index (orbit phase), or Jacobian Stretch \
                                 (a local-chaos proxy - chaos glows). \
                                 'anim -> a' / 'anim -> b' (0..1) set how much the animation \
                                 clock sweeps a/b: 0 = static, 1 = full-rate; independent, so \
                                 unequal drives trace a Lissajous path through (a,b) space and \
                                 the pattern morphs on its own (the Speed dial sets the rate; \
                                 the full beat-locked parameter orbit is Tier 2). Best in \
                                 Surface = Splat + bloom/HDR with the Inferno / Magma palette \
                                 (the 'fire'); emissive cubes show the raw shape. a/b are \
                                 host-mappable, so you can also automate them directly.");
                    });
                }

                if gmode == GeneratorMode::FieldEngine {
                    use crate::params::FieldPreset;
                    // A completed async "Load Field Program" write (sidecar on disk +
                    // `field_load_pending` set) is applied here on the GUI thread: switch
                    // to Custom, THEN bump `field_gen`. Doing both from the same GUI frame —
                    // preset first — guarantees `process()` never packs a new `field_gen`
                    // alongside a stale gallery preset (which would make the visual
                    // recompile the gallery and ignore the freshly written sidecar). The
                    // `field_gen` edge also forces a recompile when the file is re-loaded
                    // while already on Custom (preset unchanged). A cancelled dialog sets
                    // neither, so the current phenomenon is untouched.
                    if field_load_pending.swap(false, Ordering::Relaxed) {
                        setter.begin_set_parameter(&params.field_preset);
                        setter.set_parameter(&params.field_preset, FieldPreset::Custom);
                        setter.end_set_parameter(&params.field_preset);
                        field_gen.fetch_add(1, Ordering::Relaxed);
                    }
                    card(&mut c[0], "Field Engine (#381)", |ui| {
                        param_combo_sized(ui, w0, "phenomenon", &params.field_preset, setter, 2.0 * COMBO_W);
                        param_combo_sized(ui, w0, "render kind", &params.field_kind, setter, 2.0 * COMBO_W);
                        if ui
                            .button("Load Field Program (.txt)…")
                            .on_hover_text(
                                "Load a custom field expression over (x,y,z,t) — e.g. \
                                 `charge(a,0,0,0)`, `curl` of an analytic potential, \
                                 `a*(x+i*y)*exp(-0.5*r)`. Sets phenomenon = Custom and \
                                 hot-reloads (like the Network JSON). Vocabulary: + - * / ^, \
                                 sin cos exp log sqrt abs tanh, dot cross norm normalize vec, \
                                 re im conj, charge/dipole/vortex/planewave/gaussian, and the \
                                 live coefficients a/b (host-mappable).",
                            )
                            .clicked()
                        {
                            // The async thread writes the sidecar and sets
                            // `field_load_pending`; the GUI loop above then switches the
                            // phenomenon to Custom and bumps `field_gen` together — only
                            // once the new program is on disk.
                            pick_field_program_async(field_load_pending.clone());
                        }
                        if ui
                            .button("Load Field Clip… (.bin)")
                            .on_hover_text(
                                "#407 Field Playback: load a pre-baked, downsampled \
                                 physics-field clip (derived offline from PolymathicAI's \
                                 The Well datasets). Set phenomenon's PDE preset to \
                                 'Playback (Dataset)' to replay it through the lattice \
                                 glyphs, stepped off the beat clock. Opens the installed \
                                 field gallery.",
                            )
                            .clicked()
                        {
                            pick_field_clip_async(fieldclip_gen.clone());
                        }
                        srow(ui, w0, "domain scale k", &params.field_scale, setter);
                        srow(ui, w0, "box extent", &params.field_extent, setter);
                        srow(ui, w0, "coefficient a", &params.field_a, setter);
                        srow(ui, w0, "coefficient b", &params.field_b, setter);
                        srow(ui, w0, "seeds / resolution", &params.field_density, setter);
                        srow(ui, w0, "gain", &params.field_gain, setter);
                        srow(ui, w0, "thickness", &params.field_thickness, setter);
                        ui.label(egui::RichText::new("— time-marched dynamics (#381/#407) —").weak().small());
                        param_combo_sized(ui, w0, "PDE preset", &params.pde_preset, setter, 2.0 * COMBO_W);
                        // Tier B (#407): Neural CA source — a learned surrogate rolled out
                        // live on a CPU grid. The dropdown picks it; this button loads a
                        // trained weights JSON (else the built-in default renders).
                        if ui
                            .button("Load NCA Model (JSON)…")
                            .on_hover_text(
                                "Load a trained Neural Cellular Automaton (#407 Tier B) — \
                                 {channels,hidden,w1,b1,w2,b2} weights (row-major). Trained \
                                 offline on PolymathicAI's The Well (Gray-Scott / active-matter). \
                                 Set PDE preset = 'Neural CA (Learned)' to render it. With no \
                                 file loaded a built-in default (a λ–ω reaction-diffusion \
                                 oscillator) rolls out, so it always shows something. Opens at \
                                 the installed NCA gallery.",
                            )
                            .clicked()
                        {
                            pick_nca_async(nca_gen.clone());
                        }
                        help(ui, "Render an arbitrary CLOSED-FORM field equation (#381 Tier 1). \
                                 A tiny expression evaluator over (x,y,z,t) returns a scalar φ, \
                                 vector F, or complex ψ; the render kind picks the viz — Vector = \
                                 field-lines + particle aura (like Maxwell/Acoustic), Scalar = a \
                                 density/height glyph lattice, Complex = |ψ|² density tinted by \
                                 phase arg ψ. Pick a Phenomenon (Coulomb, dipole, ABC flow, a \
                                 hydrogen orbital, plane wave, vortex, Gaussian) or Load a custom \
                                 program. The coefficients a/b are bound to the program's `a`/`b` \
                                 and are host-mappable/automatable — automate a Coulomb charge or \
                                 a plane-wave ω from an Ableton clip. Streamlines topology, so \
                                 every Surface mode / material applies. Best in Swept Tubes / \
                                 Glass + bloom/HDR.");
                    });
                }

                // #354: the Duo-Field synth "Sound" card moved to its own Synth tab.

                if gmode == GeneratorMode::AxonWaveguide {
                    card(&mut c[0], "Axon Waveguide", |ui| {
                        srow(ui, w0, "fibres", &params.ax_count, setter);
                        srow(ui, w0, "length", &params.ax_length, setter);
                        srow(ui, w0, "bundle radius", &params.ax_bundle, setter);
                        srow(ui, w0, "samples/fibre", &params.ax_samples, setter);
                        srow(ui, w0, "thickness", &params.ax_thickness, setter);
                        srow(ui, w0, "splay", &params.ax_splay, setter);
                        srow(ui, w0, "seed", &params.ax_seed, setter);
                        ui.label(egui::RichText::new("— Ranvier nodes —").weak().small());
                        srow(ui, w0, "node spacing", &params.ax_node_spacing, setter);
                        srow(ui, w0, "node pinch", &params.ax_node_dip, setter);
                        ui.label(egui::RichText::new("— action-potential pulse —").weak().small());
                        srow(ui, w0, "pulse speed", &params.ax_pulse_speed, setter);
                        srow(ui, w0, "pulse width", &params.ax_pulse_width, setter);
                        srow(ui, w0, "stagger", &params.ax_stagger, setter);
                        ui.label(egui::RichText::new("— brain tract —").weak().small());
                        srow(ui, w0, "curve (C-arc)", &params.ax_curve, setter);
                        srow(ui, w0, "tortuosity", &params.ax_tortuosity, setter);
                        srow(ui, w0, "bend / scatter", &params.ax_bend, setter);
                        srow(ui, w0, "DTI colour", &params.ax_dti, setter);
                        ui.label(egui::RichText::new("— guided mode —").weak().small());
                        param_combo_sized(ui, w0, "mode", &params.ax_mode, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "mode amount", &params.ax_mode_amount, setter);
                        srow(ui, w0, "dispersion", &params.ax_dispersion, setter);
                        srow(ui, w0, "polarization", &params.ax_polarization, setter);
                        help(ui, "A bundle of myelinated axons as optical waveguides — a nerve \
                                 fibre is a step-index fibre-optic (myelin n~1.44 over axoplasm \
                                 n~1.38). View it in Swept Tubes + Glass/Refractive for the \
                                 guided-light look. Ranvier nodes pinch the sheath periodically; \
                                 an emissive pulse runs each fibre, staggered into a travelling \
                                 wave. The guided mode lights the bundle cross-section (LP01 a \
                                 centre core, LP11 two lobes, LP02 a core + ring…) — mode amount \
                                 0 = uniform. Bend/scatter arcs the bundle and makes the \
                                 edge-riding modes leak + flare at the nodes while the LP01 core \
                                 survives (straight = coherent, bent = outer modes scatter). \
                                 Dispersion chirps the pulse into a chromatic spread; \
                                 polarization shimmers the core coherently and scrambles the \
                                 leaking fibres to noise. Best with bloom/HDR.");
                    });
                }

                if gmode == GeneratorMode::NeuralNetwork {
                    neural_network_card(&mut c[0], w0, &params, setter, &nn_gen);
                }

                if gmode == GeneratorMode::Synchrotron {
                    card(&mut c[0], "Synchrotron Radiation", |ui| {
                        param_combo_sized(ui, w0, "view", &params.sy_view, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "orbit radius", &params.sy_radius, setter);
                        srow(ui, w0, "beta (v/c)", &params.sy_beta, setter);
                        srow(ui, w0, "charges", &params.sy_charges, setter);
                        srow(ui, w0, "orbit tilt °", &params.sy_tilt, setter);
                        srow(ui, w0, "precession", &params.sy_precess, setter);
                        srow(ui, w0, "near-field", &params.sy_near, setter);
                        srow(ui, w0, "thickness", &params.sy_thickness, setter);
                        srow(ui, w0, "source clamp", &params.sy_rmin, setter);
                        ui.label(egui::RichText::new("— field arrows —").weak().small());
                        srow(ui, w0, "grid", &params.sy_grid, setter);
                        srow(ui, w0, "plane extent", &params.sy_extent, setter);
                        srow(ui, w0, "arrow gain", &params.sy_amp, setter);
                        crow(ui, "perpendicular plane", &params.sy_perp, setter);
                        ui.label(egui::RichText::new("— field lines —").weak().small());
                        srow(ui, w0, "line seeds", &params.sy_line_seeds, setter);
                        srow(ui, w0, "line steps", &params.sy_line_steps, setter);
                        srow(ui, w0, "line step ds", &params.sy_line_ds, setter);
                        srow(ui, w0, "line bound", &params.sy_line_bound, setter);
                        ui.label(egui::RichText::new("— field volume —").weak().small());
                        srow(ui, w0, "volume layers", &params.sy_vol_layers, setter);
                        crow(ui, "invert (inside-out)", &params.sy_invert, setter);
                        srow(ui, w0, "invert radius", &params.sy_invert_radius, setter);
                        ui.label(egui::RichText::new("— reveal (arrows + volume) —").weak().small());
                        srow(ui, w0, "reveal (cull weak)", &params.sy_reveal, setter);
                        help(ui, "The Liénard–Wiechert field of charge(s) orbiting a circle, \
                                 solved at the retarded time of the moving source — the \
                                 velocity (1/R²) + relativistically beamed radiation (1/R) \
                                 terms, whose lobe sweeps a searchlight spiral as the charge \
                                 orbits (rides Speed + the beat). View = field arrows on a \
                                 plane, traced E field lines (seeded around the orbit, the \
                                 more organic look), or a field volume (the arrow plane \
                                 extruded into a 3-D box — heavier, drop grid/layers). \
                                 Reveal culls the dead low-field crust so only the active \
                                 core/lobes/spiral show; Invert turns the volume inside-out \
                                 (sphere inversion) so the dense core fills the view. Orbit \
                                 tilt + precession tip and cone the orbit plane so the whole \
                                 field tumbles through 3-D instead of one plane (precession is \
                                 a fraction of the max safe rate; it stays sub-luminal). Push \
                                 beta toward 1 to sharpen the beam; \
                                 near-field = 0 keeps the pure radiation spiral; bunch charges \
                                 for interference. Best in Swept Tubes / Flow-Aligned + bloom/HDR.");
                    });
                }

                if gmode == GeneratorMode::VectorField {
                    card(&mut c[0], "Vector Field", |ui| {
                        param_combo(ui, w0, "view", &params.vf_view, setter);
                        param_combo(ui, w0, "function", &params.vf_preset, setter);
                        srow(ui, w0, "extent", &params.vf_extent, setter);
                        srow(ui, w0, "field scale", &params.vf_field_scale, setter);
                        srow(ui, w0, "evolve", &params.vf_evolve, setter);
                        srow(ui, w0, "z lift", &params.vf_z_lift, setter);
                        ui.label(egui::RichText::new("— arrows —").weak().small());
                        srow(ui, w0, "grid x", &params.vf_grid_x, setter);
                        srow(ui, w0, "grid y", &params.vf_grid_y, setter);
                        srow(ui, w0, "grid z", &params.vf_grid_z, setter);
                        srow(ui, w0, "arrow gain", &params.vf_amp, setter);
                        srow(ui, w0, "thickness", &params.vf_thickness, setter);
                        param_combo(ui, w0, "length map", &params.vf_mag_map, setter);
                        param_combo(ui, w0, "tint", &params.vf_tint_mode, setter);
                        srow(ui, w0, "reveal (cull weak)", &params.vf_reveal, setter);
                        use crate::params::{VecFieldPreset, VecTermFunc};
                        if params.vf_preset.value() == VecFieldPreset::Custom {
                            // #173 Tier 3: the function builder — each
                            // component of F = 3 terms of
                            // gain·func(a·x + b·y + c·z + phase).
                            ui.label(egui::RichText::new("— function builder —").weak().small());
                            param_combo(ui, w0, "operator", &params.vb_op, setter);
                            srow(ui, w0, "helmholtz mix", &params.vb_mix, setter);
                            let rows: [(&str, [(&EnumParam<VecTermFunc>, &FloatParam, &FloatParam, &FloatParam, &FloatParam, &FloatParam); 3]); 3] = [
                                ("Fx", [
                                    (&params.vb_x1_func, &params.vb_x1_gain, &params.vb_x1_a, &params.vb_x1_b, &params.vb_x1_c, &params.vb_x1_phase),
                                    (&params.vb_x2_func, &params.vb_x2_gain, &params.vb_x2_a, &params.vb_x2_b, &params.vb_x2_c, &params.vb_x2_phase),
                                    (&params.vb_x3_func, &params.vb_x3_gain, &params.vb_x3_a, &params.vb_x3_b, &params.vb_x3_c, &params.vb_x3_phase),
                                ]),
                                ("Fy", [
                                    (&params.vb_y1_func, &params.vb_y1_gain, &params.vb_y1_a, &params.vb_y1_b, &params.vb_y1_c, &params.vb_y1_phase),
                                    (&params.vb_y2_func, &params.vb_y2_gain, &params.vb_y2_a, &params.vb_y2_b, &params.vb_y2_c, &params.vb_y2_phase),
                                    (&params.vb_y3_func, &params.vb_y3_gain, &params.vb_y3_a, &params.vb_y3_b, &params.vb_y3_c, &params.vb_y3_phase),
                                ]),
                                ("Fz", [
                                    (&params.vb_z1_func, &params.vb_z1_gain, &params.vb_z1_a, &params.vb_z1_b, &params.vb_z1_c, &params.vb_z1_phase),
                                    (&params.vb_z2_func, &params.vb_z2_gain, &params.vb_z2_a, &params.vb_z2_b, &params.vb_z2_c, &params.vb_z2_phase),
                                    (&params.vb_z3_func, &params.vb_z3_gain, &params.vb_z3_a, &params.vb_z3_b, &params.vb_z3_c, &params.vb_z3_phase),
                                ]),
                            ];
                            for (axis, terms) in rows {
                                ui.label(egui::RichText::new(format!("{axis} = t1 + t2 + t3")).weak().small());
                                for (t, (func, gain, a, b, c, phase)) in terms.into_iter().enumerate() {
                                    // Collapse silent terms to one row so
                                    // the card stays scannable.
                                    param_combo(ui, w0, &format!("{axis} t{} func", t + 1), func, setter);
                                    if func.value() != VecTermFunc::Off {
                                        srow(ui, w0, "  gain", gain, setter);
                                        srow(ui, w0, "  arg x", a, setter);
                                        srow(ui, w0, "  arg y", b, setter);
                                        srow(ui, w0, "  arg z", c, setter);
                                        srow(ui, w0, "  phase", phase, setter);
                                    }
                                }
                            }
                        }
                        ui.label(egui::RichText::new("— field lines —").weak().small());
                        param_combo(ui, w0, "seeding", &params.vf_seed_mode, setter);
                        srow(ui, w0, "line seeds", &params.vf_line_seeds, setter);
                        srow(ui, w0, "line steps", &params.vf_line_steps, setter);
                        srow(ui, w0, "line step ds", &params.vf_line_ds, setter);
                        crow(ui, "bidirectional", &params.vf_bidir, setter);
                        param_combo(ui, w0, "line colour", &params.vf_line_color, setter);
                        srow(ui, w0, "line thickness", &params.vf_line_thickness, setter);
                        srow(ui, w0, "flow pulse", &params.vf_flow, setter);
                        srow(ui, w0, "flow speed", &params.vf_flow_speed, setter);
                        ui.label(
                            egui::RichText::new(
                                "Plot a function F(x, y, z) — the vector-field classic, \
                                 in 3-D. View = the arrow lattice, its traced field \
                                 lines (the reel's \"filling it in\" shot), both \
                                 (faint arrows under the lines), or a stream surface: \
                                 equal-length lines traced from a seed curve (Ring \
                                 seeding = a closed drum, others = a straight curtain) \
                                 that Membrane mode lofts into a flowing sheet — the \
                                 line seeds/steps/ds/colour/flow controls drive it too. \
                                 The bank's first two \
                                 functions are the reel's (y², −x²) and (sin y, sin x); \
                                 set grid z = 1 for the flat 2-D plot. Field lines: \
                                 RK4 streamlines from a seed set — lattice/random/ring/\
                                 plane/|F|-weighted; bidirectional tracing joins both \
                                 directions through each seed (essential for saddles); \
                                 flow pulse marches brightness along every line off the \
                                 global Speed, so the field visibly transports on the \
                                 beat. Evolve rotates the whole field; z lift weaves the \
                                 planar classics through the volume; length map tames \
                                 1/r² poles (log). Best in Swept Tubes / Flow-Aligned + \
                                 bloom/HDR; Stream surface + Membrane + Glass = the \
                                 flowing veil.",
                            )
                            .weak()
                            .small(),
                        );
                    });
                }

                // --- Scenery column (#187 pivot) --------------------
                // Column 1: the concurrent scenery category (Zone =
                // the corridor). Column 2: its OWN material + surface
                // FX, independent of the main Look cards.
                {
                    use crate::params::SceneryMode;
                    let w1 = c[1].available_width() - COL_PAD;
                    let scm = params.sc_mode.value();
                    card(&mut c[1], "Scenery", |ui| {
                        param_combo_sized(ui, w1, "type", &params.sc_mode, setter, 2.0 * COMBO_W);
                        if scm != SceneryMode::None {
                            param_combo(ui, w1, "surface", &params.sc_surface, setter);
                        }
                        ui.label(
                            egui::RichText::new(
                                "Generated scenery, CONCURRENT with the generator: \
                                 the world you move through, with its own material \
                                 and palette (column 3). Zone = the beat-locked \
                                 corridor; Terra = flowing landscapes (fjords / \
                                 river banks / canyons). The generator rides along \
                                 inside it (set the generator to None for the pure \
                                 ride). Surface = Skin lofts the scenery into a \
                                 solid membrane (best for Terra); Cubes/Rods/Tubes \
                                 draw its lattice.",
                            )
                            .weak()
                            .small(),
                        );
                    });
                    if scm == SceneryMode::Zone {
                        card(&mut c[1], "Zone Corridor", |ui| {
                            use crate::params::RailArchetype;
                            param_combo(ui, w1, "archetype", &params.rl_archetype, setter);
                            let arch = params.rl_archetype.value();
                            srow(ui, w1, "speed (units/beat)", &params.rl_speed, setter);
                            srow(ui, w1, "bore radius", &params.rl_bore, setter);
                            srow(ui, w1, "horizon (beats)", &params.rl_horizon, setter);
                            ui.label(egui::RichText::new("— morph cells —").weak().small());
                            param_combo(ui, w1, "cell length", &params.rl_cell_len, setter);
                            param_combo(ui, w1, "change every", &params.rl_change_every, setter);
                            srow(ui, w1, "variance", &params.rl_variance, setter);
                            srow(ui, w1, "evolve", &params.rl_evolve, setter);
                            srow(ui, w1, "seed", &params.rl_seed, setter);
                            ui.label(egui::RichText::new("— profile —").weak().small());
                            srow(ui, w1, "swell", &params.rl_swell, setter);
                            if matches!(
                                arch,
                                RailArchetype::Throat
                                    | RailArchetype::Gates
                                    | RailArchetype::Waveguide
                            ) {
                                // Waveguide reads this as the azimuthal mode number m.
                                srow(ui, w1, "max lobes / mode m", &params.rl_lobes, setter);
                            }
                            srow(ui, w1, "spikiness", &params.rl_spike, setter);
                            srow(ui, w1, "twist (turns/beat)", &params.rl_twist, setter);
                            if arch == RailArchetype::PhylloWall {
                                srow(ui, w1, "divergence (deg)", &params.rl_diverge, setter);
                                srow(ui, w1, "parastichy", &params.rl_parastichy, setter);
                            }
                            if arch == RailArchetype::TissueTube {
                                srow(ui, w1, "shells", &params.rl_shells, setter);
                            }
                            ui.label(egui::RichText::new("— wall —").weak().small());
                            srow(ui, w1, "ring count", &params.rl_ring_n, setter);
                            srow(ui, w1, "rows / beat", &params.rl_rows_beat, setter);
                            srow(ui, w1, "thickness", &params.rl_thickness, setter);
                            srow(ui, w1, "beat ribs", &params.rl_rib_gain, setter);
                            srow(ui, w1, "fade-in (beats)", &params.rl_fade, setter);
                            srow(ui, w1, "colour flow", &params.rl_color_flow, setter);
                            ui.label(
                                egui::RichText::new(
                                    "Fly forward forever (#187): the rail coordinate \
                                     IS the beat clock, so cell seams and the rib \
                                     rows are crossed exactly on the beat at any \
                                     speed (speed only stretches space). Archetypes: \
                                     the superformula Throat; the Phyllo Wall \
                                     (detune divergence to re-lace; 8/13/21 = \
                                     Fibonacci); Rings & Gates (minor gates every \
                                     cell/4, MAJOR on the boundary — cell = bar for \
                                     a gate per beat); the Tissue Tube (nested \
                                     counter-rotating shells); the Tiling Liner \
                                     (Truchet mosaic — Swept Tubes); Flow Media \
                                     (ink streamers — swell = wander); the Waveguide \
                                     (traveling TE mode — lobes = mode m, twist = \
                                     cycles/beat). Drag the visual to steer inside \
                                     the bore; ribs flash even with Pulse off; route \
                                     Pulse → Rail Speed to pump motion. Geometry \
                                     changes latch and enter at the horizon so you \
                                     fly into them exactly on the next 'change \
                                     every' boundary; Evolve re-rolls the ride per \
                                     phrase. Factory 'Rails —' presets are ready to \
                                     map to Key Map notes.",
                                )
                                .weak()
                                .small(),
                            );
                        });
                    }
                    if scm == SceneryMode::Terra {
                        card(&mut c[1], "Terra Landscape", |ui| {
                            param_combo(ui, w1, "form", &params.terra_form, setter);
                            srow(ui, w1, "speed (units/beat)", &params.rl_speed, setter);
                            srow(ui, w1, "world scale (bore)", &params.rl_bore, setter);
                            srow(ui, w1, "horizon (beats)", &params.rl_horizon, setter);
                            srow(ui, w1, "lateral res (ring)", &params.rl_ring_n, setter);
                            srow(ui, w1, "rows / beat", &params.rl_rows_beat, setter);
                            ui.label(egui::RichText::new("— morph cells —").weak().small());
                            param_combo(ui, w1, "cell length", &params.rl_cell_len, setter);
                            param_combo(ui, w1, "change every", &params.rl_change_every, setter);
                            srow(ui, w1, "variance", &params.rl_variance, setter);
                            srow(ui, w1, "evolve", &params.rl_evolve, setter);
                            srow(ui, w1, "seed", &params.rl_seed, setter);
                            ui.label(egui::RichText::new("— landform —").weak().small());
                            srow(ui, w1, "ridge height", &params.terra_ridge, setter);
                            srow(ui, w1, "channel width", &params.terra_channel, setter);
                            srow(ui, w1, "valley width", &params.terra_width, setter);
                            srow(ui, w1, "steepness", &params.terra_steep, setter);
                            srow(ui, w1, "terracing", &params.terra_terrace, setter);
                            srow(ui, w1, "roughness", &params.terra_rough, setter);
                            srow(ui, w1, "detail freq", &params.terra_noise_freq, setter);
                            srow(ui, w1, "meander", &params.terra_meander, setter);
                            ui.label(egui::RichText::new("— water —").weak().small());
                            crow(ui, "water", &params.terra_water_on, setter);
                            srow(ui, w1, "water level", &params.terra_water_level, setter);
                            srow(ui, w1, "clearance", &params.terra_clearance, setter);
                            ui.label(
                                egui::RichText::new(
                                    "Fly forever through flowing landscapes (#206): \
                                     a continuous fBm heightfield whose shape morphs \
                                     per cell (no tiles — contiguous by construction, \
                                     C¹ seams). Form biases fjord / river / canyon; \
                                     the channel MEANDERS (the world sweeps under a \
                                     straight-flying camera) and the navigable \
                                     channel is always open (clearance). Best with \
                                     surface = Skin. Landform changes latch and enter \
                                     at the horizon — fly into a new landform exactly \
                                     on the 'change every' bar; Evolve re-rolls it per \
                                     phrase. Water level drives the channel water \
                                     sheet (Scenery Water card) + a shore tint band.",
                                )
                                .weak()
                                .small(),
                            );
                        });
                        // Shown whenever Terra is the scenery mode (not
                        // gated on the live `water_on`): the sheet is built
                        // from the LATCHED terra block, so water can still
                        // render for a bar after toggling off — keep the
                        // controls available (#227 review).
                        {
                            card(&mut c[1], "Scenery Water", |ui| {
                                // The water is a dedicated physical-water
                                // surface (its own shading), so it has no
                                // material-TYPE selector — `wt_mat` would be
                                // ignored by the shader (#227 review). Its
                                // look is the dials below.
                                srow(ui, w1, "roughness", &params.wt_roughness, setter);
                                srow(ui, w1, "glass ior", &params.wt_ior, setter);
                                srow(ui, w1, "opacity", &params.wt_opacity, setter);
                                srow(ui, w1, "glow", &params.wt_glow, setter);
                                srow(ui, w1, "ripple", &params.wt_ripple, setter);
                                srow(ui, w1, "ripple freq", &params.wt_ripple_freq, setter);
                                ui.label(egui::RichText::new("— physical water —").weak().small());
                                srow(ui, w1, "depth absorption", &params.wt_absorb, setter);
                                srow(ui, w1, "sun glitter", &params.wt_glitter, setter);
                                srow(ui, w1, "reflectivity", &params.wt_reflect, setter);
                                ui.label(
                                    egui::RichText::new(
                                        "The channel water (#206 Tier 3): a rippled \
                                         sheet at the per-cell water level, spanning \
                                         the valley, with its OWN material (default \
                                         Glass). The banks occlude it at the shoreline. \
                                         It joins the FX prepass, so SSR reflects the \
                                         fjord walls in it. Ripple scrolls with the \
                                         beat clock. Set water level in Terra Landscape.",
                                    )
                                    .weak()
                                    .small(),
                                );
                            });
                        }
                    }
                    if scm != SceneryMode::None {
                        let w2 = c[2].available_width() - COL_PAD;
                        card(&mut c[2], "Scenery Material", |ui| {
                            param_combo(ui, w2, "material", &params.sc_mat, setter);
                            srow(ui, w2, "metallic", &params.sc_metallic, setter);
                            srow(ui, w2, "roughness", &params.sc_roughness, setter);
                            srow(ui, w2, "glass ior", &params.sc_ior, setter);
                            srow(ui, w2, "glow", &params.sc_glow, setter);
                            srow(ui, w2, "emissive (HDR)", &params.sc_emissive, setter);
                            srow(ui, w2, "opacity", &params.sc_opacity, setter);
                            ui.label(egui::RichText::new("— colour: hue / sat / value (#305) —").weak().small());
                            srow(ui, w2, "hue", &params.scen_hue, setter);
                            srow(ui, w2, "hue cycle /beat", &params.scen_hue_cycle, setter);
                            srow(ui, w2, "saturation", &params.scen_saturation, setter);
                            srow(ui, w2, "value", &params.scen_value, setter);
                            ui.label(
                                egui::RichText::new(
                                    "The scenery's OWN substance — independent of \
                                     the Look tab's Material card, which keeps \
                                     shading the primary generator.",
                                )
                                .weak()
                                .small(),
                            );
                        });
                        card(&mut c[2], "Scenery Surface FX", |ui| {
                            param_combo(ui, w2, "palette", &params.sc_palette, setter);
                            srow(ui, w2, "translucency", &params.sc_sss, setter);
                            srow(ui, w2, "sss distortion", &params.sc_sss_dist, setter);
                            srow(ui, w2, "sss power", &params.sc_sss_pow, setter);
                            srow(ui, w2, "iridescence", &params.sc_irid, setter);
                            srow(ui, w2, "irid scale", &params.sc_irid_scale, setter);
                            srow(ui, w2, "irid hue", &params.sc_irid_shift, setter);
                        });
                    }
                }

                if gmode == GeneratorMode::Phyllotaxis {
                    card(&mut c[0], "Phyllotaxis", |ui| {
                        param_combo_sized(ui, w0, "surface", &params.phyl_surface, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "count", &params.phyl_count, setter);
                        srow(ui, w0, "divergence °", &params.phyl_divergence, setter);
                        srow(ui, w0, "radius", &params.phyl_radius, setter);
                        srow(ui, w0, "parastichy", &params.phyl_parastichy, setter);
                        srow(ui, w0, "height", &params.phyl_height, setter);
                        srow(ui, w0, "shell growth", &params.phyl_growth, setter);
                        srow(ui, w0, "breathe amp", &params.phyl_breathe_amp, setter);
                        srow(ui, w0, "breathe freq", &params.phyl_breathe_freq, setter);
                        srow(ui, w0, "rotation", &params.phyl_rot, setter);
                        srow(ui, w0, "thickness", &params.phyl_thickness, setter);
                        help(ui, "Golden-angle node placement — Vogel's sunflower (disk), \
                                 Fibonacci sphere, cone, or log-spiral shell. The \
                                 parastichy spiral families are the strands (set it to a \
                                 Fibonacci number — 13/21/34 — for clean spirals); \
                                 detuning the divergence off 137.5° is dramatic. Rotation \
                                 + radius breathing ride the global Speed. Grid topology: \
                                 Membrane skins a spiral ribbon.");
                    });
                }

                if gmode == GeneratorMode::Tessellation {
                    card(&mut c[0], "Tessellation", |ui| {
                        param_combo_sized(ui, w0, "family", &params.tess_family, setter, 2.0 * COMBO_W);
                        param_combo_sized(ui, w0, "construction", &params.tess_construct, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "depth", &params.tess_depth, setter);
                        srow(ui, w0, "grid range", &params.tess_grid_n, setter);
                        srow(ui, w0, "scale", &params.tess_scale, setter);
                        param_combo_sized(ui, w0, "view", &params.tess_view, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "edge thickness", &params.tess_thickness, setter);
                        srow(ui, w0, "extrude height (×size)", &params.tess_height, setter);
                        param_combo_sized(ui, w0, "height mode", &params.tess_height_mode, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "phason", &params.tess_phason, setter);
                        srow(ui, w0, "Ammann bars", &params.tess_ammann, setter);
                        srow(ui, w0, "hyperbolic p", &params.tess_hyp_p, setter);
                        srow(ui, w0, "hyperbolic q", &params.tess_hyp_q, setter);
                        srow(ui, w0, "beat inflate", &params.tess_beat_infl, setter);
                        srow(ui, w0, "beat ripple", &params.tess_ripple_amt, setter);
                        srow(ui, w0, "ripple freq", &params.tess_ripple_freq, setter);
                        help(ui, "Aperiodic tilings as real geometry — the discrete cousin \
                                 of the KIFS field. Families: Penrose, Ammann–Beenker, \
                                 Pinwheel (infinitely many angles), Truchet (arcs → flowing \
                                 labyrinths), Hyperbolic {p,q} (Escher Circle-Limit in the \
                                 Poincaré disk — needs 1/p+1/q < 1/2, e.g. 7,3). \
                                 Construction: Inflation (depth = the dial) or Cut-and-project \
                                 (de Bruijn multigrid; grid range = the dial) — unlocks \
                                 Ammann–Beenker + Phason (slide the window → endless \
                                 rearrangement) + Ammann bars (the grid lines themselves, on \
                                 the Edges view). View: Edges / Filled / Extruded prisms / \
                                 3-D quasicrystal (honest Z⁶ rod lattice). Filled & Extruded \
                                 → Material = Glass/Chrome + a Palette. With Pulse on, beat \
                                 inflate + ripple animate. (Hat/Spectre einstein is the one \
                                 remaining follow-up.)");
                    });
                }

                if gmode == GeneratorMode::Mandelbulb {
                    card(&mut c[0], "Mandelbulb", |ui| {
                        srow(ui, w0, "power", &params.mb_power, setter);
                        srow(ui, w0, "iterations", &params.mb_iter, setter);
                        srow(ui, w0, "scale", &params.mb_scale, setter);
                        srow(ui, w0, "detail (steps)", &params.mb_detail, setter);
                        srow(ui, w0, "spin", &params.mb_spin, setter);
                        srow(ui, w0, "morph", &params.mb_morph, setter);
                        srow(ui, w0, "colour", &params.mb_color, setter);
                        srow(ui, w0, "bailout", &params.mb_bailout, setter);
                        help(ui, "A distance-estimated 3-D fractal (White–Nylander \
                                 power-8 set), raymarched per pixel rather than built \
                                 from nodes — so the Surface mode/palette don't apply, \
                                 but it shares the full PBR/IBL/HDR/bloom/camera stack. \
                                 Power morphs the lobing (gorgeous automated/pulsed); \
                                 spin + morph ride the global Speed (and Speed Pulse), \
                                 so it tumbles + unfolds on the beat, and Breath swells \
                                 it. Iterations/detail trade fractal crispness for GPU \
                                 cost — drop them on a projector.");
                    });
                }

                if gmode == GeneratorMode::Creature {
                    card(&mut c[0], "Creature Engine", |ui| {
                        if ui
                            .button("Load Creature (JSON)…")
                            .on_hover_text(
                                "Load an authored body plan — {name?, emit:[ ... ]} \
                                 where each emitter is {kind:\"ellipsoid\",center,radii,\
                                 k,glow} / {kind:\"cone\",a,b,r0,r1,k,glow} / \
                                 {kind:\"paddle\",center,half,round,k,glow} / \
                                 {kind:\"chain\",of,count,a,b,size0,size1,k,glow}. Any \
                                 emitter may set mirror_x:true for a bilateral pair. \
                                 Replaces the built-in form until you pick another.",
                            )
                            .clicked()
                        {
                            pick_creature_async(creature_gen.clone());
                        }
                        srow(ui, w0, "form", &params.cr_form, setter);
                        srow(ui, w0, "scale", &params.cr_scale, setter);
                        srow(ui, w0, "detail (steps)", &params.cr_detail, setter);
                        srow(ui, w0, "swim", &params.cr_swim, setter);
                        srow(ui, w0, "swim amp", &params.cr_warp_amp, setter);
                        srow(ui, w0, "swim freq", &params.cr_warp_freq, setter);
                        srow(ui, w0, "rim glow", &params.cr_rim, setter);
                        srow(ui, w0, "bioluminescence", &params.cr_glow, setter);
                        srow(ui, w0, "band amount", &params.cr_wave_amt, setter);
                        srow(ui, w0, "band speed", &params.cr_wave_speed, setter);
                        srow(ui, w0, "band count", &params.cr_wave_freq, setter);
                        srow(ui, w0, "band sharpness", &params.cr_wave_sharp, setter);
                        crow(ui, "anatomy overlay", &params.cr_overlay, setter);
                        srow(ui, w0, "overlay opacity", &params.cr_overlay_opacity, setter);
                        srow(ui, w0, "overlay brightness", &params.cr_overlay_bright, setter);
                        help(ui, "A synthetic sea creature assembled from a union of \
                                 signed-distance primitives (ellipsoids, tapered \
                                 capsules, paddles) placed along a spine, raymarched \
                                 per pixel rather than built from nodes — so the \
                                 Surface mode/palette don't apply, but it shares the \
                                 full PBR/IBL/HDR/bloom/camera stack. Form picks the \
                                 body plan (0 bell jelly, 1 ribbon-swimmer, 2 \
                                 paddle-finned predator); swim rides the global Speed \
                                 (and Speed Pulse), so the peristaltic undulation \
                                 pulses on the beat. Detail trades silhouette crispness \
                                 for GPU cost — drop it on a projector.");
                    });
                }

                if gmode == GeneratorMode::Lens {
                    card(&mut c[0], "Lens (#258)", |ui| {
                        srow(ui, w0, "focal / curvature", &params.lens_focal, setter);
                        srow(ui, w0, "aperture", &params.lens_aperture, setter);
                        srow(ui, w0, "thickness", &params.lens_thickness, setter);
                        crow(ui, "plano-convex (else biconvex)", &params.lens_plano, setter);
                        srow(ui, w0, "scale", &params.lens_scale, setter);
                        srow(ui, w0, "detail (steps)", &params.lens_detail, setter);
                        help(ui, "An analytic double-convex / plano-convex lens, raymarched \
                                 per pixel as a signed distance field (the intersection of \
                                 two spheres, or a sphere with a flat face, clipped by the \
                                 aperture) — so the Surface mode/palette don't apply, but it \
                                 shares the full PBR/IBL/HDR/bloom/camera stack. Focal sets \
                                 the cap curvature (sphere radius = focal × scale); aperture \
                                 and thickness are fractions of the world size. Shade it as \
                                 Glass/Refractive to refract the environment through it.");
                    });
                }

                if gmode == GeneratorMode::NeuralField {
                    card(&mut c[0], "Neural Field (#200)", |ui| {
                        srow(ui, w0, "seed A", &params.neural_seed_a, setter);
                        srow(ui, w0, "seed B", &params.neural_seed_b, setter);
                        srow(ui, w0, "latent walk", &params.neural_walk, setter);
                        srow(ui, w0, "walk rate (beats)", &params.neural_walk_rate, setter);
                        srow(ui, w0, "field size", &params.neural_scale, setter);
                        srow(ui, w0, "detail", &params.neural_coord, setter);
                        srow(ui, w0, "feature scale", &params.neural_omega, setter);
                        srow(ui, w0, "iso", &params.neural_iso, setter);
                        srow(ui, w0, "steps", &params.neural_steps, setter);
                        srow(ui, w0, "surface smooth", &params.neural_march, setter);
                        srow(ui, w0, "colour", &params.neural_color, setter);
                        ui.label(egui::RichText::new("— strand form (Tier 1b) —").weak().small());
                        crow(ui, "strand form", &params.neural_strands_mode, setter);
                        srow(ui, w0, "columns", &params.neural_strands_cols, setter);
                        srow(ui, w0, "rows", &params.neural_strands_rows, setter);
                        srow(ui, w0, "extent", &params.neural_strands_extent, setter);
                        srow(ui, w0, "displace", &params.neural_strands_displace, setter);
                        // Neural acceleration detection (#200 Tier 2).
                        ui.label(
                            egui::RichText::new(format!(
                                "accel: coop-matrix {} · f16 {} · Metal island {}{}",
                                if coopmat_available { "✓" } else { "✕" },
                                if f16_available { "✓" } else { "✕" },
                                if island_available { "✓" } else { "✕" },
                                if island_available && island_gflops > 0.0 {
                                    format!(" ({island_gflops:.0} GFLOPs)")
                                } else {
                                    String::new()
                                },
                            ))
                            .weak()
                            .small(),
                        );
                        help(ui, "A tiny SIREN MLP (x,y,z,t) → (density, colour) \
                                 raymarched per pixel as an isosurface — no nodes, so \
                                 the Surface mode/palette don't apply, but it shares the \
                                 full PBR/IBL/HDR/bloom/camera stack. The whole organism \
                                 is SEED A; step it for a completely different creature. \
                                 LATENT WALK morphs from seed A to seed B (a smooth \
                                 weight blend); WALK RATE drives that morph off the beat \
                                 clock (0 = manual). Field size + detail + feature scale \
                                 set the organism's size and how busy it is; iso carves \
                                 how much is solid; steps trades accuracy for GPU cost \
                                 (drop it on a projector); surface smooth softens the \
                                 normals. Breath swells it, the camera orbits it. \
                                 STRAND FORM (Tier 1b) instead samples the same network \
                                 on a columns×rows grid and DISPLACES the nodes into a \
                                 rippling sheet — now it's a real node field, so every \
                                 Surface mode (cubes / tubes / membrane) + Material + the \
                                 palette apply; extent sets the sheet size, displace how \
                                 far the density pushes each node out.");
                    });
                }

                if gmode == GeneratorMode::MinimalSurface {
                    // Implicit families (TPMS + bubbles/foam → raymarch) and
                    // parametric Weierstrass families (Grid) have different
                    // effectors, so show only the ones that act on the chosen
                    // family. (`ms_parametric` computed above, with the Surface
                    // gate — parametric = Enneper/Catenoid/Helicoid (3..5),
                    // bubbles/foam 6,7 are implicit.)
                    card(&mut c[0], "Minimal surfaces", |ui| {
                        // Shared across both kinds.
                        param_combo_sized(ui, w0, "family", &params.ms_family, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "scale", &params.ms_scale, setter);
                        srow(ui, w0, "thickness", &params.ms_thickness, setter);
                        srow(ui, w0, "colour", &params.ms_color, setter);
                        srow(ui, w0, "twist", &params.ms_twist, setter);
                        if ms_parametric {
                            // Parametric (Enneper / Catenoid / Helicoid).
                            srow(ui, w0, "bend speed", &params.ms_bend, setter);
                            srow(ui, w0, "bend phase", &params.ms_bend_phase, setter);
                            srow(ui, w0, "turns", &params.ms_turns, setter);
                            srow(ui, w0, "uv resolution", &params.ms_uv_res, setter);
                            srow(ui, w0, "extent", &params.ms_extent, setter);
                            help(ui, "Parametric surfaces — built as a (u,v) grid, so every \
                                     Surface mode + Material skins them like the bell (Glass \
                                     + a little iridescence = a soap film). WEIERSTRASS \
                                     (Enneper / Catenoid / Helicoid, H = 0): Catenoid + \
                                     Helicoid are one associate family — bend speed bends a \
                                     catenoid through a helicoid (the isometric \
                                     deformation), bend phase parks a static blend; turns = \
                                     u-revolutions; extent = how much surface to sample. \
                                     CMC (Unduloid / Nodoid, constant mean curvature — \
                                     bubble-chains / liquid bridges): bend phase = neck \
                                     (pinched sphere-chain → cylinder; bigger loops for the \
                                     nodoid), bend speed pulses it (mean-curvature flow — \
                                     the bulges breathe), turns = number of bulges. Twist \
                                     adds a helical warp; uv resolution = smoothness.");
                        } else {
                            // Implicit raymarch (Gyroid / Schwarz P / D +
                            // Bubbles / Foam).
                            srow(ui, w0, "cells", &params.ms_cells, setter);
                            srow(ui, w0, "isolevel", &params.ms_iso, setter);
                            srow(ui, w0, "detail (steps)", &params.ms_detail, setter);
                            srow(ui, w0, "beat isolevel", &params.ms_beat_iso, setter);
                            srow(ui, w0, "form resolution", &params.ms_form_res, setter);
                            help(ui, "Implicit isosurfaces, raymarched per pixel (Surface \
                                     mode/palette don't apply, but the PBR/IBL/HDR/Glass \
                                     stack does). TPMS (Gyroid / Schwarz P / D) are H ≈ 0 \
                                     minimal surfaces; Bubbles (merged soap spheres) and \
                                     Foam (Voronoi Plateau-border walls) are soap geometry \
                                     with built-in thin-film rainbows; and the algebraic \
                                     bank (Clebsch cubic / Barth sextic / Kummer quartic / \
                                     Heart / Tanglecube) is a gallery of classic polynomial \
                                     surfaces — try Material = Glass on those. Isolevel \
                                     swells / pinches the surface — for bubbles that's the \
                                     size, so beat isolevel makes them pulse and merge on \
                                     the beat (Pulse on); cells = labyrinth / bubble-count \
                                     fineness (inert for the algebraic surfaces); thickness \
                                     = film/wall width; twist shears the domain; detail \
                                     trades crispness for GPU cost; form resolution is the \
                                     perf lever.");
                        }
                    });
                }

                if gmode == GeneratorMode::Kaleidoscope {
                    card(&mut c[0], "Kaleidoscopic Fractal", |ui| {
                        param_combo_sized(ui, w0, "space", &params.kf_space, setter, 2.0 * COMBO_W);
                        param_combo_sized(ui, w0, "pattern", &params.kf_pattern, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "sectors", &params.kf_sectors, setter);
                        srow(ui, w0, "fold (c)", &params.kf_fold, setter);
                        srow(ui, w0, "iterations", &params.kf_iter, setter);
                        srow(ui, w0, "iter rotation", &params.kf_iter_rot, setter);
                        srow(ui, w0, "warp", &params.kf_warp, setter);
                        srow(ui, w0, "churn", &params.kf_churn, setter);
                        srow(ui, w0, "E8 8-D rotation", &params.kf_e8_flow, setter);
                        srow(ui, w0, "spin", &params.kf_spin, setter);
                        srow(ui, w0, "breathe", &params.kf_breathe, setter);
                        srow(ui, w0, "zoom", &params.kf_zoom, setter);
                        srow(ui, w0, "tunnel", &params.kf_tunnel, setter);
                        srow(ui, w0, "tunnel flow", &params.kf_flow, setter);
                        param_combo_sized(ui, w0, "3D mode", &params.kf_view, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "relief height", &params.kf_relief, setter);
                        srow(ui, w0, "3D elevation", &params.kf_relief_elev, setter);
                        srow(ui, w0, "3D steps", &params.kf_relief_steps, setter);
                        srow(ui, w0, "3D shine", &params.kf_relief_shine, setter);
                        srow(ui, w0, "rays", &params.kf_rays, setter);
                        srow(ui, w0, "ring", &params.kf_ring, setter);
                        srow(ui, w0, "petals", &params.kf_petals, setter);
                        srow(ui, w0, "sharpness", &params.kf_sharp, setter);
                        srow(ui, w0, "glow", &params.kf_glow, setter);
                        srow(ui, w0, "contrast", &params.kf_contrast, setter);
                        srow(ui, w0, "invert", &params.kf_invert, setter);
                        srow(ui, w0, "dispersion", &params.kf_dispersion, setter);
                        param_combo_sized(ui, w0, "palette", &params.kf_palette, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "hue", &params.kf_hue, setter);
                        srow(ui, w0, "colour speed", &params.kf_color_speed, setter);
                        help(ui, "N-fold kaleidoscopic symmetry feeding a selectable \
                                 fractal engine (pattern). Fold (c) is the big shape \
                                 lever (beautiful automated/pulsed — it re-grows the \
                                 fractal); warp adds an organic swirl; spin + breathe \
                                 ride the global Speed (and Speed Pulse). Tunnel wraps \
                                 the field around a receding bore (tunnel flow flies \
                                 you down it). Palette + colour speed drive the colour \
                                 scheme + its cycling; invert flips figure↔ground and \
                                 dispersion splits the edges into a prism rim.");
                    });
                }

                if gmode == GeneratorMode::Boids {
                    card(&mut c[0], "Boids (flocking)", |ui| {
                        param_combo_sized(ui, w0, "form", &params.boids_form, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "creature size", &params.boids_size, setter);
                        srow(ui, w0, "banking", &params.boids_bank, setter);
                        srow(ui, w0, "count", &params.boids_count, setter);
                        srow(ui, w0, "perception", &params.boids_perception, setter);
                        srow(ui, w0, "separation", &params.boids_separation, setter);
                        srow(ui, w0, "sep weight", &params.boids_sep, setter);
                        srow(ui, w0, "align weight", &params.boids_align, setter);
                        srow(ui, w0, "cohere weight", &params.boids_cohere, setter);
                        srow(ui, w0, "max speed", &params.boids_max_speed, setter);
                        srow(ui, w0, "max force", &params.boids_max_force, setter);
                        srow(ui, w0, "trail", &params.boids_trail, setter);
                        srow(ui, w0, "bounds", &params.boids_bounds, setter);
                        srow(ui, w0, "goal pull", &params.boids_goal, setter);
                        srow(ui, w0, "sim speed", &params.boids_speed, setter);
                        srow(ui, w0, "scale", &params.boids_scale, setter);
                        srow(ui, w0, "thickness", &params.boids_thickness, setter);
                        srow(ui, w0, "seed", &params.boids_seed, setter);
                        help(ui, "N agents obeying Reynolds' local rules — separation \
                                 (anti-collision), alignment (match heading), cohesion \
                                 (steer to the local centre) — inside a soft bounding \
                                 sphere. Coherence with no central metronome: the flock \
                                 IS the animation. Goal pull gathers the flock to the \
                                 centre; with Pulse on it pulses on the beat (gather / \
                                 scatter). Sim speed rides the global Speed. Form draws \
                                 each agent as a fish / bird / manta / dart (oriented by \
                                 velocity, banking into turns) — overriding the surface \
                                 mode; Surface keeps the normal cubes / tubes / metaball \
                                 (Streamlines topology).");
                    });
                }

                if gmode == GeneratorMode::Demo {
                    card(&mut c[0], "Demo (scene bench)", |ui| {
                        param_combo_sized(ui, w0, "scene", &params.demo_scene, setter, 2.0 * COMBO_W);
                        srow(ui, w0, "scale", &params.demo_size, setter);
                        crow(ui, "inner objects", &params.demo_objects, setter);
                        crow(ui, "fixed camera", &params.demo_static_cam, setter);
                        srow(ui, w0, "light", &params.demo_light, setter);
                        srow(ui, w0, "roughness", &params.demo_roughness, setter);
                        srow(ui, w0, "count", &params.demo_count, setter);
                        srow(ui, w0, "spin", &params.demo_spin, setter);
                        help(ui, "A hand-authored REFERENCE SCENE for the ray-tracing \
                                 stack — Cornell box, sphere pyramids, a glass menagerie, \
                                 a light stage. It emits explicit instanced geometry, so it \
                                 casts + receives shadows, feeds the TLAS, and the path \
                                 tracer (P) renders it immediately (the Cornell box is the \
                                 path tracer's ground-truth scene). Per-primitive materials \
                                 put a mirror sphere next to a glass sphere next to diffuse \
                                 coloured walls in one frame; the light stage adds placeable \
                                 emitters. Scale sizes the scene; inner objects toggles the \
                                 hero shapes; light drives the emitter/key; count sets the \
                                 pyramid rows / grid side / rig lights; spin turns the \
                                 turntable on the beat; fixed camera holds the front-on \
                                 reference framing (turn it off to orbit).");
                    });
                }

            }); // end Generator-tab columns
            } // end Generator tab

            // ── Motion tab ─────────────────────────────────────────
            if state.tab == preset::UiTab::Motion {
            fixed_columns(ui, |c| {
                let w1 = (c[0].available_width() - COL_PAD).max(150.0);
                card(&mut c[0], "Animation", |ui| {
                    srow(ui, w1, "animate", &params.animate, setter);
                    srow(ui, w1, "speed (global)", &params.inc_scale, setter);
                    srow(ui, w1, "speed power", &params.speed_exp, setter);
                });
                card(&mut c[0], "Camera (Auto-Orbit)", |ui| {
                    param_combo(ui, w1, "path", &params.cam_path, setter);
                    srow(ui, w1, "flow speed", &params.cam_speed, setter);
                    srow(ui, w1, "kick", &params.cam_kick, setter);
                    srow(ui, w1, "damping", &params.cam_damping, setter);
                    srow(ui, w1, "amount", &params.cam_amount, setter);
                    srow(ui, w1, "beat momentum", &params.cam_beat_momentum, setter);
                    help(ui, "Beat momentum on = the camera lurches on the beat (today's \
                             feel). Off = it glides smoothly on the bar clock — cinematic, \
                             no wiggle with the audio.");
                });
                card(&mut c[0], "Camera Sequence (#307)", |ui| {
                    srow(ui, w1, "sequencer", &params.cam_seq_enabled, setter);
                    param_combo(ui, w1, "bars/shot", &params.cam_bars_per_shot, setter);
                    param_combo(ui, w1, "order", &params.cam_seq_order, setter);
                    param_combo(ui, w1, "transition", &params.cam_transition, setter);
                    srow(ui, w1, "glide bars", &params.cam_transition_bars, setter);
                    help(ui, "Progress through the camera moves on musical bar marks \
                             instead of holding one path. Series cycles them; Random picks \
                             (no immediate repeat). Glide eases between shots over 'glide \
                             bars'; Cut snaps on the downbeat.");
                    srow(ui, w1, "hold chance", &params.cam_hold_prob, setter);
                    srow(ui, w1, "phrase lock", &params.cam_phrase_lock, setter);
                    srow(ui, w1, "blend (orbit↔seq)", &params.cam_seq_mix, setter);
                    help(ui, "Hold chance repeats a shot sometimes (less predictable); \
                             phrase lock snaps each move to a canonical facing on the \
                             downbeat. Blend mixes the always-on orbit-cam (the 'path' \
                             above) with the sequencer: 0 = fully orbit-cam, 1 = fully \
                             sequencer. Flow speed drives both.");
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(
                        "Clock source + beats/bar → Settings › Sync / Tempo",
                    ).weak().small());
                });
                card(&mut c[0], "Camera Dolly (#307)", |ui| {
                    srow(ui, w1, "period (bars)", &params.cam_dolly_period, setter);
                    srow(ui, w1, "depth", &params.cam_dolly_depth, setter);
                    param_combo(ui, w1, "wave", &params.cam_dolly_wave, setter);
                    help(ui, "An in/out radius breath on its own bar period, independent \
                             of the orbit speed — a slow wide orbit can still breathe. \
                             Depth 0 = off.");
                    ui.add_space(4.0);
                    srow(ui, w1, "roll (dutch)", &params.cam_roll, setter);
                    srow(ui, w1, "field of view", &params.cam_fov, setter);
                    srow(ui, w1, "dolly zoom", &params.cam_fov_dolly, setter);
                    help(ui, "Roll tilts the horizon (dutch angle); FOV is the base lens; \
                             dolly zoom couples FOV to the dolly for a Hitchcock vertigo \
                             warp (push in → widen). All inert at roll 0 / FOV 45 / zoom 0.");
                });
                card(&mut c[0], "Camera Storyboard (#307)", |ui| {
                    srow(ui, w1, "storyboard", &params.cam_story_enabled, setter);
                    srow(ui, w1, "shots", &params.cam_story_count, setter);
                    param_combo(ui, w1, "order", &params.cam_story_mode, setter);
                    srow(ui, w1, "seed", &params.cam_story_seed, setter);
                    if ui
                        .button("next shot ▶")
                        .on_hover_text("Advance to the next storyboard shot at the next bar")
                        .clicked()
                    {
                        story_next_gen.fetch_add(1, Ordering::Relaxed);
                    }
                    ui.add_space(4.0);
                    for (i, (pp, bb, rr)) in [
                        (&params.cam_shot0_path, &params.cam_shot0_bars, &params.cam_shot0_radius),
                        (&params.cam_shot1_path, &params.cam_shot1_bars, &params.cam_shot1_radius),
                        (&params.cam_shot2_path, &params.cam_shot2_bars, &params.cam_shot2_radius),
                        (&params.cam_shot3_path, &params.cam_shot3_bars, &params.cam_shot3_radius),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        // Scope each shot's widgets under a unique id so the
                        // per-shot combos (which key on their label string via
                        // ComboBox::from_id_salt) don't collide across rows —
                        // otherwise only the last "move"/"bars" dropdown is
                        // interactive and hovering it highlights all four.
                        ui.push_id(i, |ui| {
                            ui.label(egui::RichText::new(format!("shot {}", i + 1)).weak().small());
                            param_combo(ui, w1, "move", pp, setter);
                            param_combo(ui, w1, "bars", bb, setter);
                            srow(ui, w1, "radius", rr, setter);
                        });
                    }
                    help(ui, "An authored playlist of shots (move + bars + framing radius) \
                             that overrides the auto sequencer. Series/Random/Shuffle/\
                             Weighted with a seed for reproducibility; 'next shot' advances \
                             on the next bar. Off → the sequencer / single path is unchanged.");
                });
                card(&mut c[0], "Pulse", |ui| {
                    ui.label(egui::RichText::new(
                        "Pulse, tempo, clock source + audio detection are now in one \
                         place → Settings › Sync / Tempo.",
                    ).weak().small());
                });
                card(&mut c[1], "Speed Pulse", |ui| {
                    srow(ui, w1, "amount", &params.speed_pulse_amount, setter);
                    srow(ui, w1, "attack", &params.speed_pulse_attack, setter);
                    srow(ui, w1, "decay", &params.speed_pulse_decay, setter);
                    help(ui, "Logarithmic kick to the rotation speed on each pulse — \
                             amount is in powers of 10 (1 = ×10, e.g. 10⁻³→10⁻²). Use \
                             this for speed, not the Rotation Speed routing target \
                             (linear, invisible at small speeds). Needs Pulse on.");
                });
                card(&mut c[1], "Breath", |ui| {
                    srow(ui, w1, "amount", &params.breath_amount, setter);
                    srow(ui, w1, "attack", &params.breath_attack, setter);
                    srow(ui, w1, "decay", &params.breath_decay, setter);
                    help(ui, "Universal pulse-driven scale of the whole scene about its \
                             centre — works for every generator + surface mode (it \
                             breathes against a fixed sky). A full pulse swells the \
                             scene by × (1 + amount). Needs Pulse on.");
                });
            }); // end Motion-tab columns
            } // end Motion tab

            // ── Environment tab ────────────────────────────────────
            // The world layer (was the floating 🌍 panel): per-display,
            // not preset-captured — so no per-tab preset list.
            if state.tab == preset::UiTab::Environment {
            fixed_columns(ui, |c| {
                let we = (c[0].available_width() - COL_PAD).max(150.0);
                environment_ui(c, we, &params, setter);
            }); // end Environment-tab columns
            } // end Environment tab

            // ── Look tab ───────────────────────────────────────────
            if state.tab == preset::UiTab::Look {
            fixed_columns(ui, |c| {
                let w2 = (c[0].available_width() - COL_PAD).max(150.0);
                // (The Renderer + Output Resolution cards moved to the
                // Settings tab — per-display plumbing, not a look.)
                // Surface: how nodes become geometry — leads the Look
                // column (moved from the Generator tab). Node-field
                // generators only (raymarch generators have no nodes).
                if !raymarch {
                card(&mut c[0], "Surface", |ui| {
                    param_combo_sized(ui, w2, "mode", &params.surface_mode, setter, 2.0 * COMBO_W);
                    param_combo_sized(ui, w2, "palette", &params.palette, setter, 2.0 * COMBO_W);
                    help(ui, "Palette = colour LUT for the sweep. Native keeps the current \
                             look; any other applies across all surface modes.");
                    // Node bevel: rounds the cube geometry (Original +
                    // Flow-Aligned) from a sharp cube through a rounded cube
                    // to a full sphere. Only these two modes draw cubes.
                    if matches!(
                        params.surface_mode.value(),
                        crate::params::HostSurfaceMode::Original
                            | crate::params::HostSurfaceMode::FlowAligned
                    ) {
                        srow(ui, w2, "node bevel", &params.bevel, setter);
                        help(ui, "Rounds the cubes: 0 = sharp cube, 0.5 = wide \
                                 rounded cube, 1 = sphere. In Flow-Aligned the \
                                 rods round into smooth capsules.");
                        // #472 Tier 1: procedural / texture-mapped PBR materials.
                        // A folder of PNGs (albedo/normal/roughness/metallic/ao/
                        // height) sampled onto the generator cubes; off = today.
                        ui.separator();
                        crow(ui, "material maps", &params.mat_enable, setter);
                        if params.mat_enable.value() {
                            // Only texture-MAPPING controls live here (how the maps
                            // project + tile). The material qualities themselves —
                            // roughness / metallic / normal / AO — come straight from
                            // the loaded maps into the ONE material system (the main
                            // Material card's roughness/metallic drive any channel a
                            // set doesn't provide), so there are no per-map quality knobs.
                            param_combo_sized(ui, w2, "projection", &params.mat_projection, setter, 2.0 * COMBO_W);
                            srow(ui, w2, "mat scale", &params.mat_scale, setter);
                            if ui
                                .button("Load Material…")
                                .on_hover_text(
                                    "Pick a folder of PBR PNGs \
                                     (albedo/normal/roughness/metallic/ao/height)",
                                )
                                .clicked()
                            {
                                pick_material_async(material_gen.clone());
                            }
                            help(ui, "Loads a folder of PBR maps as the surface's albedo / \
                                     normal / roughness / metallic / AO — feeding the full \
                                     material system, so the Material type (Chrome / Glass / \
                                     Subsurface / …), GI, and reflections all apply on top. \
                                     Triplanar needs no UVs; an absent channel falls back to \
                                     the scalar Material sliders. Off = today's look.");
                        }
                        // #472 Tier 2: procedural noise layer — baked into
                        // the routed channel by a compute pass (supersedes
                        // the PNG for that channel). Off = today's look.
                        ui.separator();
                        crow(ui, "procedural (noise)", &params.mp_enable, setter);
                        // #472 Tier 4: the declarative material.json graph —
                        // human/agent-authorable, shareable, gallery-installed.
                        ui.horizontal(|ui| {
                            if ui
                                .button("Load Material Graph…")
                                .on_hover_text(
                                    "Load a material.json (procedural noise graph) — \
                                     turns materials + procedural on and applies it.",
                                )
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Material Graph", &["json"])
                                    .set_directory(preset::material_graphs_dir())
                                    .pick_file()
                                {
                                    if let Ok(txt) = std::fs::read_to_string(&path) {
                                        if let Ok(g) =
                                            crate::material_graph::MaterialGraph::from_json(&txt)
                                        {
                                            g.apply(&params, setter);
                                        }
                                    }
                                }
                            }
                            if ui
                                .button("Save Material Graph…")
                                .on_hover_text("Write the current material as a material.json graph")
                                .clicked()
                            {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Material Graph", &["json"])
                                    .set_directory(preset::material_graphs_dir())
                                    .set_file_name("material.json")
                                    .save_file()
                                {
                                    let g = crate::material_graph::MaterialGraph::from_params(&params);
                                    let _ = std::fs::write(&path, g.to_json());
                                }
                            }
                        });
                        if params.mp_enable.value() {
                            param_combo_sized(ui, w2, "noise", &params.mp_noise, setter, 2.0 * COMBO_W);
                            param_combo_sized(ui, w2, "→ channel", &params.mp_channel, setter, 2.0 * COMBO_W);
                            srow(ui, w2, "noise scale", &params.mp_scale, setter);
                            srow(ui, w2, "rotation", &params.mp_rotation, setter);
                            srow(ui, w2, "offset X", &params.mp_offset_x, setter);
                            srow(ui, w2, "offset Y", &params.mp_offset_y, setter);
                            srow(ui, w2, "octaves", &params.mp_octaves, setter);
                            srow(ui, w2, "lacunarity", &params.mp_lacunarity, setter);
                            srow(ui, w2, "gain", &params.mp_gain, setter);
                            srow(ui, w2, "domain warp", &params.mp_warp, setter);
                            srow(ui, w2, "contrast", &params.mp_contrast, setter);
                            srow(ui, w2, "gamma", &params.mp_gamma, setter);
                            srow(ui, w2, "remap low", &params.mp_remap_lo, setter);
                            srow(ui, w2, "remap high", &params.mp_remap_hi, setter);
                            crow(ui, "invert", &params.mp_invert, setter);
                            srow(ui, w2, "seed", &params.mp_seed, setter);
                            param_combo_sized(ui, w2, "bake res", &params.mp_res, setter, 2.0 * COMBO_W);
                            if params.mp_channel.value()
                                == crate::params::MatChannel::Albedo
                            {
                                srow(ui, w2, "grad low R", &params.mp_lo_r, setter);
                                srow(ui, w2, "grad low G", &params.mp_lo_g, setter);
                                srow(ui, w2, "grad low B", &params.mp_lo_b, setter);
                                srow(ui, w2, "grad high R", &params.mp_hi_r, setter);
                                srow(ui, w2, "grad high G", &params.mp_hi_g, setter);
                                srow(ui, w2, "grad high B", &params.mp_hi_b, setter);
                            }
                            help(ui, "Bakes a noise field into the chosen channel \
                                     (albedo maps through the gradient; others write a \
                                     scalar). FBM/Turbulence/Ridged use the octave/\
                                     lacunarity/gain dials; scale snaps to whole tiles \
                                     so an un-rotated bake tiles seamlessly.");
                            // #472 Tier 3: overlay layer 2 (composites onto
                            // layer 1's output for the same channel).
                            ui.separator();
                            crow(ui, "layer 2", &params.mp2_enable, setter);
                            if params.mp2_enable.value() {
                                param_combo_sized(ui, w2, "L2 noise", &params.mp2_noise, setter, 2.0 * COMBO_W);
                                param_combo_sized(ui, w2, "L2 → channel", &params.mp2_channel, setter, 2.0 * COMBO_W);
                                param_combo_sized(ui, w2, "L2 blend", &params.mp2_blend, setter, 2.0 * COMBO_W);
                                srow(ui, w2, "L2 scale", &params.mp2_scale, setter);
                                srow(ui, w2, "L2 rotation", &params.mp2_rotation, setter);
                                srow(ui, w2, "L2 offset X", &params.mp2_offset_x, setter);
                                srow(ui, w2, "L2 offset Y", &params.mp2_offset_y, setter);
                                srow(ui, w2, "L2 octaves", &params.mp2_octaves, setter);
                                srow(ui, w2, "L2 lacunarity", &params.mp2_lacunarity, setter);
                                srow(ui, w2, "L2 gain", &params.mp2_gain, setter);
                                srow(ui, w2, "L2 warp", &params.mp2_warp, setter);
                                srow(ui, w2, "L2 contrast", &params.mp2_contrast, setter);
                                srow(ui, w2, "L2 gamma", &params.mp2_gamma, setter);
                                srow(ui, w2, "L2 remap low", &params.mp2_remap_lo, setter);
                                srow(ui, w2, "L2 remap high", &params.mp2_remap_hi, setter);
                                crow(ui, "L2 invert", &params.mp2_invert, setter);
                                srow(ui, w2, "L2 seed", &params.mp2_seed, setter);
                                if params.mp2_channel.value()
                                    == crate::params::MatChannel::Albedo
                                {
                                    srow(ui, w2, "L2 grad low R", &params.mp2_lo_r, setter);
                                    srow(ui, w2, "L2 grad low G", &params.mp2_lo_g, setter);
                                    srow(ui, w2, "L2 grad low B", &params.mp2_lo_b, setter);
                                    srow(ui, w2, "L2 grad high R", &params.mp2_hi_r, setter);
                                    srow(ui, w2, "L2 grad high G", &params.mp2_hi_g, setter);
                                    srow(ui, w2, "L2 grad high B", &params.mp2_hi_b, setter);
                                }
                            }
                            // #472 Tier 3: derived maps (the correlation
                            // principle — normal + AO agree with the surface).
                            ui.separator();
                            crow(ui, "derive normal", &params.mat_derive_normal, setter);
                            crow(ui, "derive AO", &params.mat_derive_ao, setter);
                            if params.mat_derive_normal.value()
                                || params.mat_derive_ao.value()
                            {
                                crow(ui, "normal from albedo", &params.mat_normal_source_albedo, setter);
                                srow(ui, w2, "normal strength", &params.mat_derive_normal_strength, setter);
                                srow(ui, w2, "AO strength", &params.mat_derive_ao_strength, setter);
                                srow(ui, w2, "AO radius", &params.mat_derive_ao_radius, setter);
                                help(ui, "Derives a normal (Sobel of the height, or albedo \
                                         luminance) and/or AO (cavity of the height) so the \
                                         maps agree — bake a Height layer, then derive.");
                            }
                            // #472 Tier 5: temporal animation (re-bakes the
                            // procedural layers each frame at a throttled rate)
                            // + height→vertex displacement (geometry, not just
                            // shading). Both inert at their defaults.
                            ui.separator();
                            crow(ui, "animate", &params.mat_anim_enable, setter);
                            if params.mat_anim_enable.value() {
                                param_combo_sized(ui, w2, "motion", &params.mat_anim_mode, setter, 2.0 * COMBO_W);
                                srow(ui, w2, "anim speed", &params.mat_anim_speed, setter);
                                if params.mat_anim_mode.value()
                                    == crate::params::AnimMode::Drift
                                {
                                    srow(ui, w2, "flow X", &params.mat_flow_x, setter);
                                    srow(ui, w2, "flow Y", &params.mat_flow_y, setter);
                                }
                                help(ui, "Drift pans the noise (flow X/Y), Evolve morphs \
                                         it in place, Rotate spins each layer. Re-bakes at \
                                         ~30 Hz — cost scales with layer count.");
                            }
                            srow(ui, w2, "height displace", &params.mat_displace, setter);
                            help(ui, "Pushes vertices along the surface normal by the baked \
                                     Height field (needs a Height layer). 0 = flat (shading \
                                     only). Real geometry — casts shadows, catches silhouettes.");
                        }
                    }
                    // Metaball-only: wrap the node set in one smooth skin.
                    // Shown only in Metaball mode (was always visible).
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Metaball
                    {
                        srow(ui, w2, "blob radius", &params.metaball_radius, setter);
                        srow(ui, w2, "blob threshold", &params.metaball_threshold, setter);
                        srow(ui, w2, "blob smoothness", &params.metaball_smooth, setter);
                        help(ui, "Radius must exceed node spacing for blobs to fuse \
                                 into a contiguous skin.");
                    }
                    // Splat-only: render the node set as anisotropic 3-D
                    // Gaussians (the 3DGS primitive, synthesized directly).
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Splat
                    {
                        param_combo_sized(ui, w2, "tier", &params.splat_mode, setter, 2.0 * COMBO_W);
                        srow(ui, w2, "splat radius", &params.splat_radius, setter);
                        srow(ui, w2, "splat opacity", &params.splat_opacity, setter);
                        srow(ui, w2, "splat falloff", &params.splat_falloff, setter);
                        srow(ui, w2, "splat cutoff", &params.splat_cutoff, setter);
                        srow(ui, w2, "splat anisotropy", &params.splat_aniso, setter);
                        srow(ui, w2, "splat scatter", &params.splat_scatter, setter);
                        srow(ui, w2, "splat jitter", &params.splat_jitter, setter);
                        srow(ui, w2, "splat solidity", &params.splat_solid, setter);
                        help(ui, "Each node becomes an anisotropic Gaussian (no \
                                 photogrammetry — synthesized from the node's matrix). \
                                 Additive = unlit glow that blooms through HDR; Lit = \
                                 sorted 2DGS disks shaded by the IBL + the Material card \
                                 (Chrome/Glass reflect + refract the environment). \
                                 Solidity 0 = soft Gaussian bokeh → 1 = opaque discs: \
                                 raise it (with Opacity ~1 + enough Radius for the discs \
                                 to meet) for a compact opaque SURFACE instead of a blur. \
                                 Scatter sprays jittered sub-splats per node for density.");
                    }
                    // Swept-Tubes-only: weld the per-segment cylinders into
                    // one smooth continuous tube per strand, with a shaped cap.
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::SweptTubes
                    {
                        crow(ui, "contiguous tube", &params.tube_weld, setter);
                        srow(ui, w2, "tube profile", &params.tube_profile, setter);
                        crow(ui, "end caps", &params.tube_end_cap, setter);
                        srow(ui, w2, "cap rounding", &params.tube_cap_round, setter);
                        srow(ui, w2, "cap bevel", &params.tube_cap_bevel, setter);
                        help(ui, "'Contiguous' welds the segments into one smooth \
                                 tube per strand (closing the gaps at bends). Tube \
                                 profile 1 = round → 0 = sharp square (welded cubes). \
                                 Cap rounding 0 = flat → 1 = dome; cap bevel 0 = \
                                 rounded → 1 = chamfer.");
                    }
                    // Voxel-only: splat the node field into a lattice and
                    // DDA-raymarch crisp grid-snapped cubes.
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Voxel
                    {
                        srow(ui, w2, "voxel grid", &params.voxel_res, setter);
                        srow(ui, w2, "voxel threshold", &params.voxel_threshold, setter);
                        srow(ui, w2, "voxel radius", &params.voxel_radius, setter);
                        srow(ui, w2, "voxel emission", &params.voxel_emission, setter);
                        srow(ui, w2, "voxel AO", &params.voxel_ao, setter);
                        srow(ui, w2, "voxel shadow", &params.voxel_shadow, setter);
                        srow(ui, w2, "voxel quantize", &params.voxel_quantize, setter);
                        srow(ui, w2, "voxel beat→threshold", &params.voxel_beat, setter);
                        help(ui, "Crisp grid-snapped cubes (flat faces, voxel AO, soft \
                                 shadows). Grid = perf dial; radius = strand thickness. \
                                 Quantize posterizes colour; emission blooms.");
                        // Voxel GI (#89): cone-traced bounced colour.
                        crow(ui, "voxel GI", &params.voxel_gi, setter);
                        if params.voxel_gi.value() {
                            srow(ui, w2, "GI strength", &params.voxel_gi_strength, setter);
                            srow(ui, w2, "GI distance", &params.voxel_gi_distance, setter);
                            srow(ui, w2, "GI sky", &params.voxel_gi_sky, setter);
                            help(ui, "Cone-traces the voxel field so emissive voxels bleed \
                                     coloured light onto their neighbours + the world \
                                     (raise emission for stronger bleed). A per-frame mip \
                                     build — drop the grid on a projector.");
                        }
                    }
                    // Volume-only (#152): raymarch the metaball field as
                    // a glowing participating medium (nebula/fog).
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Volume
                    {
                        srow(ui, w2, "volume radius", &params.volume_radius, setter);
                        srow(ui, w2, "density", &params.volume_density, setter);
                        srow(ui, w2, "emission", &params.volume_emission, setter);
                        srow(ui, w2, "absorption", &params.volume_absorption, setter);
                        srow(ui, w2, "steps", &params.volume_steps, setter);
                        help(ui, "Emissive fog: density × emission glows (HDR → blooms), \
                                 absorption thins it. Radius must exceed node spacing for \
                                 a continuous cloud; steps = perf dial.");
                        // Field Volume (#348): choose what the cloud bakes.
                        ui.separator();
                        param_combo_sized(ui, w2, "field source", &params.fv_source, setter, 2.0 * COMBO_W);
                        srow(ui, w2, "smoothing", &params.fv_smooth, setter);
                        srow(ui, w2, "exposure dB", &params.fv_exposure_db, setter);
                        crow(ui, "calibrated brightness", &params.fv_calibrate, setter);
                        srow(ui, w2, "gain", &params.fv_gain, setter);
                        ui.separator();
                        crow(ui, "field lines (flow)", &params.fv_lines, setter);
                        srow(ui, w2, "line density", &params.fv_line_density, setter);
                        srow(ui, w2, "line thickness", &params.fv_line_thickness, setter);
                        help(ui, "Field lines (Acoustic / Maxwell): render the field as a dense cloud of \
                                 thin glowing streamlines of both channels (pressure/velocity or E/B) \
                                 — the tube-mode flow, without chunky tubes. Density = how many \
                                 lines; raise glow (Lighting) for luminous filaments. \n\
                                 Field source: Legacy = today's node metaball (byte-identical). \
                                 Auto = Maxwell/Acoustic bake the analytic field ENERGY (a smooth \
                                 cloud, no scraggly far-node spikes), every other generator bakes \
                                 a smoothed node kernel. Smoothing widens the node kernel; \
                                 calibrated brightness keys the glow to the measured loudness.");
                    }
                    // Membrane-only: loft sheets between adjacent strands.
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Membrane
                    {
                        param_combo_sized(ui, w2, "membrane weave", &params.membrane_weave, setter, 2.0 * COMBO_W);
                        crow(ui, "membrane: show strands", &params.membrane_show_strands, setter);
                        crow(ui, "membrane: close seam (360°)", &params.membrane_close, setter);
                        crow(ui, "membrane: skin arms", &params.membrane_arms, setter);
                        if params.membrane_arms.value() {
                            param_combo_sized(ui, w2, "arm build", &params.membrane_arm_build, setter, 2.0 * COMBO_W);
                            srow(ui, w2, "arm radius (0=auto)", &params.membrane_arm_radius, setter);
                        }
                        crow(ui, "membrane: screen-space FX", &params.membrane_fx, setter);
                        help(ui, "Skins a sheet between neighbouring strands \
                                 (sail/bell/web). Close seam bridges the last row of strands \
                                 back to the first when the form wraps a full 360°. Skin arms \
                                 skins each strand (arm) as its own closed capped finger with \
                                 gaps between arms (the volume-render hull) — built as cheap \
                                 capsule Impostors or a welded Mesh. Screen-space FX draws the \
                                 membrane into the depth prepass so VXGI (diffuse + reflections), \
                                 SSAO, SSR, SSGI, DoF and TAA apply to it — on by default; turn \
                                 off to skip the extra depth pass and keep membrane as a flat-lit sheet.");
                    }
                    // Neural Tissue (#260): living neural tissue built from
                    // closed anatomical primitives. Best paired with the
                    // Neural Network generator. Grouped by tier — soma/
                    // membrane, dendritic arbor, myelin, synapse/context.
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::NeuralTissue
                    {
                        help(ui, "Living neural tissue: soma cell bodies, capped-capsule \
                                 tracts + synaptic boutons. Pair with Generator = Neural \
                                 Network. All morphology dials below are inert at 0 (a bare \
                                 soma field) — raise them to grow the anatomy.");
                        // Tier 1 — soma + membrane.
                        srow(ui, w2, "soma size", &params.nt_soma_size, setter);
                        srow(ui, w2, "soma shape", &params.nt_soma_shape, setter);
                        srow(ui, w2, "bouton size", &params.nt_bouton_size, setter);
                        srow(ui, w2, "membrane SSS", &params.nt_membrane_sss, setter);
                        srow(ui, w2, "membrane iridescence", &params.nt_membrane_irid, setter);
                        // Tier 2 — dendritic arbor + axon morphology.
                        param_combo_sized(ui, w2, "neuron type", &params.nt_neuron_type, setter, 2.0 * COMBO_W);
                        srow(ui, w2, "dendrite density", &params.nt_dendrite_density, setter);
                        srow(ui, w2, "dendrite length", &params.nt_dendrite_length, setter);
                        srow(ui, w2, "dendrite taper", &params.nt_dendrite_taper, setter);
                        srow(ui, w2, "dendritic spines", &params.nt_spines, setter);
                        help(ui, "Dendrite density 0 = a bare soma; raise it to grow \
                                 branching arbors (type sets the class). Spines sprinkle \
                                 tiny stubs — higher detail, off by default.");
                        // Tier 3 — myelinated axons (saltatory conduction).
                        srow(ui, w2, "myelin amount", &params.nt_myelin_amount, setter);
                        srow(ui, w2, "Ranvier spacing", &params.nt_ranvier_spacing, setter);
                        srow(ui, w2, "sheath scale", &params.nt_sheath_scale, setter);
                        help(ui, "Myelin 0 = plain capped tracts; raise it to sheathe edges \
                                 as nerve fibres (fatty internodes + Ranvier constrictions). \
                                 The pulse jumps node-to-node — turn the firing sim on \
                                 (Neural Network → Signal) to see saltatory conduction.");
                        // Tier 4 — living synapse + tissue context.
                        srow(ui, w2, "synapse cleft", &params.nt_synapse_cleft, setter);
                        srow(ui, w2, "cytoplasm glow", &params.nt_synapse_glow, setter);
                        srow(ui, w2, "vesicles", &params.nt_synapse_vesicles, setter);
                        srow(ui, w2, "glia", &params.nt_glia, setter);
                        srow(ui, w2, "capillaries", &params.nt_capillary, setter);
                        help(ui, "Cleft opens a gap at each terminal; cytoplasm glow lights \
                                 somata from within (tied to activation). Vesicles burst on \
                                 each spike arrival (needs the firing sim). Glia + capillaries \
                                 sprout faint scaffolding so the network sits in tissue.");
                    }
                    // Plexus (#8): the node cloud rebuilt as a proximity
                    // web — struts between near neighbours + a marker per
                    // node. Works on ANY node-emitting generator, either as
                    // the surface itself OR as an OVERLAY (outer shell) on
                    // top of another surface.
                    crow(ui, "plexus overlay (outer shell)", &params.plexus_overlay_on, setter);
                    if params.plexus_overlay_on.value() {
                        srow(ui, w2, "shell scale", &params.plexus_shell_scale, setter);
                        srow(ui, w2, "shell depth", &params.plexus_shell_depth, setter);
                        srow(ui, w2, "shell resolution", &params.plexus_shell_bins, setter);
                        help(ui, "Wraps the plexus web as an outer SHELL() around the CURRENT surface \
                                 (Metaball, etc.) — like the Particle Aura, it reads the node cloud \
                                 without replacing it, so the base surface still renders. Shell scale \
                                 grows the cage outward; shell depth keeps the outer band of each \
                                 direction (bigger = a thicker, steadier rind — raise it if nodes \
                                 flicker); resolution sets the outline detail. It uses all the Plexus \
                                 look controls below (impostors, materials, shape morph, signal).");
                        ui.separator();
                    }
                    if params.surface_mode.value()
                        == crate::params::HostSurfaceMode::Plexus
                        || params.plexus_overlay_on.value()
                    {
                        srow(ui, w2, "link radius", &params.plexus_radius, setter);
                        srow(ui, w2, "max links / node", &params.plexus_links, setter);
                        srow(ui, w2, "strut thickness", &params.plexus_strut, setter);
                        srow(ui, w2, "node size", &params.plexus_marker, setter);
                        srow(ui, w2, "node shape (cube→sphere)", &params.plexus_node_shape, setter);
                        srow(ui, w2, "edge shape (square→circle)", &params.plexus_edge_shape, setter);
                        help(ui, "Wires each node to its nearest neighbours. All sizes are × the \
                                 field's node spacing (auto-scaled per generator), so the same \
                                 settings read consistently everywhere. Raise link radius for a \
                                 denser web. Node shape morphs the markers cube → rounded → sphere; \
                                 edge shape morphs the strut cross-section square → circle.");
                        ui.separator();
                        // Tier 2 — GPU impostors + independent materials.
                        crow(ui, "impostors (spheres + tubes)", &params.plexus_impostor, setter);
                        if params.plexus_impostor.value() {
                            crow(ui, "draw edges", &params.plexus_edges, setter);
                            srow(ui, w2, "node radius", &params.plexus_node_radius, setter);
                            srow(ui, w2, "edge radius", &params.plexus_edge_radius, setter);
                            ui.label("Node material");
                            param_combo_sized(ui, w2, "node type", &params.plexus_node_type, setter, 2.0 * COMBO_W);
                            srow(ui, w2, "node metallic", &params.plexus_node_metallic, setter);
                            srow(ui, w2, "node roughness", &params.plexus_node_rough, setter);
                            srow(ui, w2, "node IOR", &params.plexus_node_ior, setter);
                            srow(ui, w2, "node hue", &params.plexus_node_hue, setter);
                            srow(ui, w2, "node saturation", &params.plexus_node_sat, setter);
                            srow(ui, w2, "node value", &params.plexus_node_val, setter);
                            srow(ui, w2, "node emissive", &params.plexus_node_emissive, setter);
                            ui.label("Edge material");
                            param_combo_sized(ui, w2, "edge type", &params.plexus_edge_type, setter, 2.0 * COMBO_W);
                            srow(ui, w2, "edge metallic", &params.plexus_edge_metallic, setter);
                            srow(ui, w2, "edge roughness", &params.plexus_edge_rough, setter);
                            srow(ui, w2, "edge IOR", &params.plexus_edge_ior, setter);
                            srow(ui, w2, "edge hue", &params.plexus_edge_hue, setter);
                            srow(ui, w2, "edge saturation", &params.plexus_edge_sat, setter);
                            srow(ui, w2, "edge value", &params.plexus_edge_val, setter);
                            srow(ui, w2, "edge emissive", &params.plexus_edge_emissive, setter);
                            help(ui, "Nodes as sphere impostors, edges as capsule-tube impostors \
                                     — each with its OWN full material (chrome nodes on glass \
                                     filaments, emissive nodes on matte struts, whatever).");
                        }
                        ui.separator();
                        // Tier 3 — beat-driven signal propagation.
                        crow(ui, "signal propagation", &params.plexus_signal, setter);
                        if params.plexus_signal.value() {
                            srow(ui, w2, "signal speed (/beat)", &params.plexus_signal_speed, setter);
                            srow(ui, w2, "signal gain", &params.plexus_signal_gain, setter);
                            srow(ui, w2, "signal width", &params.plexus_signal_width, setter);
                            help(ui, "A bright activation shell radiates from the web centre on \
                                     the beat, firing the impostors it crosses (needs impostors \
                                     on). Speed = shells per beat.");
                        }
                    }
                });
                } // end Surface card (node-field generators only)
                // Calibrated colour (#349) — a cross-cutting tint: colour that
                // MEANS a measured level, applied across every surface mode.
                card(&mut c[0], "Calibrated Colour (#349)", |ui| {
                    param_combo_sized(ui, w2, "mode", &params.col_mode, setter, 2.0 * COMBO_W);
                    param_combo_sized(ui, w2, "LUT", &params.col_lut, setter, 2.0 * COMBO_W);
                    param_combo_sized(ui, w2, "source", &params.col_source, setter, 2.0 * COMBO_W);
                    srow(ui, w2, "low dB", &params.col_lo_db, setter);
                    srow(ui, w2, "high dB", &params.col_hi_db, setter);
                    srow(ui, w2, "amount", &params.col_amount, setter);
                    help(ui, "Calibrated = colour means a measured level via a perceptually-\
                             uniform LUT (Turbo/Viridis/Inferno/Magma), applied once so every \
                             surface mode inherits it. Field generators (Maxwell/Acoustic) \
                             colour by band dBFS; others by momentary LUFS. Aesthetic (default) \
                             → today's tint, byte-identical. (Raymarch generators — Mandelbulb/\
                             Neural/Minimal/Lens — are not tinted this way.)");
                });
                // Post-composite creative FX (#152) — screen-space, so it
                // applies to EVERY generator (incl. KIFS). Off by default.
                card(&mut c[2], "Post FX (#152)", |ui| {
                    crow(ui, "enable", &params.fx_enabled, setter);
                    if params.fx_enabled.value() {
                        param_combo(ui, w2, "style", &params.fx_style, setter);
                        srow(ui, w2, "style amount", &params.fx_style_amt, setter);
                        srow(ui, w2, "outline threshold", &params.fx_outline, setter);
                        ui.label(egui::RichText::new("— depth of field —").weak().small());
                        srow(ui, w2, "amount", &params.fx_dof, setter);
                        srow(ui, w2, "focus", &params.fx_dof_focus, setter);
                        srow(ui, w2, "range", &params.fx_dof_range, setter);
                        ui.label(egui::RichText::new("— lens / grade —").weak().small());
                        srow(ui, w2, "chromatic aberration", &params.fx_chroma, setter);
                        srow(ui, w2, "vignette", &params.fx_vignette, setter);
                        srow(ui, w2, "film grain", &params.fx_grain, setter);
                        srow(ui, w2, "saturation", &params.fx_grade_sat, setter);
                        srow(ui, w2, "contrast", &params.fx_grade_contrast, setter);
                        srow(ui, w2, "temperature", &params.fx_grade_temp, setter);
                        srow(ui, w2, "gain", &params.fx_grade_gain, setter);
                        srow(ui, w2, "feedback trails", &params.fx_feedback, setter);
                        ui.label(egui::RichText::new("— cinematic finishing (#167) —").weak().small());
                        srow(ui, w2, "halation", &params.hal_amount, setter);
                        srow(ui, w2, "halation threshold", &params.hal_threshold, setter);
                        srow(ui, w2, "halation width", &params.hal_width, setter);
                        srow(ui, w2, "halation warmth", &params.hal_warmth, setter);
                        srow(ui, w2, "lens flare", &params.lf_amount, setter);
                        srow(ui, w2, "flare ghosts", &params.lf_ghosts, setter);
                        srow(ui, w2, "flare halo", &params.lf_halo, setter);
                        srow(ui, w2, "flare streak", &params.lf_streak, setter);
                        help(ui, "Screen-space post on the final image (composite untouched). \
                                 Style: Toon / Outline / Halftone / Dither / Pixelate. DoF \
                                 uses scene depth on the node-field paths. Halation is the warm \
                                 red bleed around highlights (≠ bloom); lens flares add ghosts + \
                                 a halo ring + an anamorphic streak from the bright points. Off → \
                                 image unchanged. A captured Look.");
                    }
                });
                // Scene Kaleidoscope (#361 Tier 1): a post-stage kaleidoscopic
                // fold of the resolved HDR scene — applies to EVERY generator +
                // surface (folds the live PBR render, before bloom/composite).
                // Off by default; always visible (works even over the KIFS field).
                card(&mut c[2], "Scene Kaleidoscope (#361)", |ui| {
                    crow(ui, "enable (fold the scene)", &params.kal_on, setter);
                    param_combo(ui, w2, "mode", &params.kal_mode, setter);
                    srow(ui, w2, "sectors", &params.kal_sectors, setter);
                    srow(ui, w2, "spin", &params.kal_spin, setter);
                    srow(ui, w2, "roll", &params.kal_roll, setter);
                    srow(ui, w2, "source zoom", &params.kal_zoom, setter);
                    srow(ui, w2, "center X", &params.kal_center_x, setter);
                    srow(ui, w2, "center Y", &params.kal_center_y, setter);
                    srow(ui, w2, "twist", &params.kal_twist, setter);
                    srow(ui, w2, "tint hue", &params.kal_tint_hue, setter);
                    srow(ui, w2, "tint amount", &params.kal_tint_amt, setter);
                    srow(ui, w2, "seam soften", &params.kal_seam, setter);
                    srow(ui, w2, "mix", &params.kal_mix, setter);
                    help(ui, "Folds the fully-lit HDR scene through N-fold \
                             kaleidoscopic symmetry (before bloom/tonemap, so \
                             highlights + EDR stay physical). Mode: Full frame = each \
                             slice shows the whole frame mirror-tiled (swimmy); Wedge = \
                             the classic optical kaleidoscope (identical slices). Spin \
                             rides global Speed + the beat. Source zoom/center frame the \
                             busy part; twist adds a spiral; seam soften supersamples the \
                             mirror lines; mix crossfades against the untouched scene. \
                             Off → image unchanged. A captured Look.");
                });
                // Quantitative instrumentation (#391 Tier 1): placeable field
                // probes + an energy ledger + a Poynting-flux surface, read from
                // the same kernels the visual draws. Only meaningful on the field
                // generators (Maxwell / Acoustic / Cavity); inert (HUD off) by default.
                card(&mut c[2], "Instrumentation (#391)", |ui| {
                    crow(ui, "HUD (draw read-outs)", &params.instr_hud, setter);
                    crow(ui, "field probe", &params.instr_probe_on, setter);
                    srow(ui, w2, "probe X", &params.instr_probe_x, setter);
                    srow(ui, w2, "probe Y", &params.instr_probe_y, setter);
                    srow(ui, w2, "probe Z", &params.instr_probe_z, setter);
                    crow(ui, "energy ledger", &params.instr_ledger_on, setter);
                    srow(ui, w2, "ledger half-extent", &params.instr_ledger_half, setter);
                    srow(ui, w2, "ledger samples", &params.instr_ledger_res, setter);
                    crow(ui, "Poynting flux", &params.instr_flux_on, setter);
                    srow(ui, w2, "flux X", &params.instr_flux_x, setter);
                    srow(ui, w2, "flux Y", &params.instr_flux_y, setter);
                    srow(ui, w2, "flux Z", &params.instr_flux_z, setter);
                    srow(ui, w2, "flux patch size", &params.instr_flux_size, setter);
                    param_combo(ui, w2, "flux axis", &params.instr_flux_axis, setter);
                    srow(ui, w2, "flux samples", &params.instr_flux_res, setter);
                    crow(ui, "log probe CSV", &params.instr_csv_log, setter);
                    param_combo(ui, w2, "HUD dock", &params.instr_hud_dock, setter);
                    srow(ui, w2, "HUD size", &params.instr_hud_scale, setter);
                    srow(ui, w2, "panel opacity", &params.instr_panel_opacity, setter);
                    srow(ui, w2, "panel bevel", &params.instr_panel_bevel, setter);
                    help(ui, "Puts honest numbers beside the field (Maxwell / Acoustic \
                             / Cavity generators). The probe reads E/B (or pressure / \
                             particle-velocity) + energy + Poynting at a point; the \
                             ledger integrates the E↔B (compression↔kinetic) energy \
                             trade over a box while the total holds; the flux surface \
                             reads the power crossing a placeable patch. Every value \
                             comes from the SAME kernel the visual draws, in the sim's \
                             normalized units. CSV log appends a probe trace to a temp \
                             file. HUD off → nothing drawn (byte-identical). A captured Look.");
                });
                // KIFS is a self-contained fullscreen colour field — these
                // node/PBR-surface look cards don't apply to it. (Mandelbulb
                // + Minimal-surface still use the PBR material, so only KIFS
                // hides them.)
                if !kifs {
                card(&mut c[1], "Cast Shadows (#152)", |ui| {
                    crow(ui, "enable (shadow map)", &params.shadow_enabled, setter);
                    srow(ui, w2, "bias", &params.shadow_bias, setter);
                    srow(ui, w2, "strength", &params.shadow_strength, setter);
                    help(ui, "Off by default. A world-space depth map from the KEY light — \
                             cubes cast real shadows on each other. Raise bias if you see \
                             shadow acne (stippling), lower it if shadows detach. \
                             Instanced/cube paths only (raymarch + membrane don't cast). \
                             On an M3+ Mac, RT Shadows (Ray Tracing card) supersede this \
                             map with traced per-pixel occlusion — no bias tuning needed.");
                });
                card(&mut c[1], "Ambient Occlusion", |ui| {
                    crow(ui, "enable (depth AO)", &params.ssao, setter);
                    param_combo(ui, w2, "source", &params.ao_source, setter);
                    srow(ui, w2, "radius", &params.ssao_radius, setter);
                    srow(ui, w2, "intensity", &params.ssao_intensity, setter);
                    srow(ui, w2, "bias (GTAO)", &params.ssao_bias, setter);
                    srow(ui, w2, "RT rays", &params.rt_ao_rays, setter);
                    help(ui, "Off by default. Contact shadowing where cubes meet. Source: \
                             GTAO = screen-space horizon integration (any machine); Ray \
                             Traced (#195 T3, M3+ Macs) = short traced hemisphere rays — \
                             ground truth, no screen-space haloing, and off-screen geometry \
                             occludes; falls back to GTAO where unsupported. Radius + \
                             intensity apply to both; bias is GTAO-only; RT rays is the \
                             per-pixel ray count (pair with TAA — it integrates the noise).");
                });
                card(&mut c[0], "Material", |ui| {
                    param_combo(ui, w2, "type", &params.mat_type, setter);
                    srow(ui, w2, "metallic", &params.metallic, setter);
                    srow(ui, w2, "roughness", &params.roughness, setter);
                    srow(ui, w2, "glass IOR", &params.ior, setter);
                    srow(ui, w2, "absorption", &params.mat_absorb, setter);
                    srow(ui, w2, "glow", &params.glow, setter);
                    srow(ui, w2, "emissive (HDR)", &params.mat_emissive, setter);
                    srow(ui, w2, "opacity", &params.opacity, setter);
                    ui.label(egui::RichText::new("— colour: hue / sat / value (#305) —").weak().small());
                    srow(ui, w2, "hue", &params.mat_hue, setter);
                    srow(ui, w2, "hue cycle /beat", &params.mat_hue_cycle, setter);
                    srow(ui, w2, "saturation", &params.mat_saturation, setter);
                    srow(ui, w2, "value", &params.mat_value, setter);
                    ui.label(egui::RichText::new("— thin-film (soap/bubble, #258) —").weak().small());
                    srow(ui, w2, "film thickness (nm)", &params.film_thickness, setter);
                    srow(ui, w2, "film marbling", &params.film_thickness_var, setter);
                    srow(ui, w2, "film IOR", &params.film_ior, setter);
                    srow(ui, w2, "film drainage", &params.film_drainage, setter);
                    help(ui, "Physical thin-film interference (#258 T1): a real \
                             wavelength-resolved soap-film / bubble spectrum on the Glass \
                             reflection (and the Foam/Bubble raymarch). Thickness 0 = OFF \
                             (the legacy cosine-sheen look); raise to ~300–600 nm for the \
                             banded soap spectrum. Marbling swirls the thickness; drainage \
                             thins the top and thickens the bottom (world up). It tints the \
                             ENVIRONMENT reflection, so it reads strongest at grazing angles \
                             and with a bright env/HDR — set Material type = Glass.");
                    crow(ui, "refraction overlay", &params.refr_overlay, setter);
                    srow(ui, w2, "overlay blend", &params.refr_blend, setter);
                    help(ui, "Refractive = Glass plus Beer–Lambert absorption over each \
                             node's measured body thickness (the liquid's see-through \
                             optics on the generators): thin edges stay clear, thick \
                             bodies go murky in the node's own colour. Absorption is \
                             the strength (instanced cube/tube modes — the raymarch \
                             surfaces fall back to Glass). The OVERLAY weaves the same \
                             refraction into the other types on top of their own look: \
                             Standard's body goes glassy (roughness frosts it), Chrome \
                             opens face-on and stays mirror at grazing angles, Glass \
                             gains the measured-thickness murk. IOR + absorption drive \
                             it; blend = how far the body opens. Redundant on \
                             Refractive itself.");
                    srow(ui, w2, "screen refraction", &params.refract_ss, setter);
                    srow(ui, w2, "refraction displace", &params.refract_dist, setter);
                    help(ui, "Screen refraction (#214 T5): on the Refractive material, a \
                             post pass shows the ACTUAL scene behind each cube — neighbours \
                             and the world, displaced by the bent view ray — instead of only \
                             the environment. 0 = off (env-only, as today). Displace sets how \
                             far the ray bends before re-sampling the scene. Screen-space, so \
                             off-screen bits fall back to the env with no seam; needs the \
                             depth prepass (instanced cube/tube modes).");
                    ui.label(egui::RichText::new("— anisotropy (#214) —").weak().small());
                    srow(ui, w2, "anisotropy", &params.anisotropy, setter);
                    srow(ui, w2, "brush rotation", &params.aniso_rotation, setter);
                    crow(ui, "anisotropy overlay", &params.aniso_overlay, setter);
                    srow(ui, w2, "overlay blend", &params.aniso_blend, setter);
                    help(ui, "Anisotropic = a brushed/streaked specular highlight instead of \
                             a round one (brushed metal, satin, hair). The streak follows \
                             each node's long axis — Swept Tubes comb along their length — \
                             and brush rotation re-aims it (best on cubes). Amount sets the \
                             strength/direction (− streaks across, + along; 0 = isotropic). \
                             The OVERLAY layers the same elliptical lobe onto Standard \
                             (satin) and Chrome (brushed chrome — the showpiece) instead of \
                             only the dedicated Anisotropic type; blend fades it in. \
                             Instanced cube/tube modes; raymarch surfaces stay isotropic.");
                    ui.label(egui::RichText::new("— surface lobes (#214 T2) —").weak().small());
                    srow(ui, w2, "clearcoat", &params.clearcoat, setter);
                    srow(ui, w2, "clearcoat roughness", &params.clearcoat_rough, setter);
                    crow(ui, "clearcoat overlay", &params.clearcoat_overlay, setter);
                    srow(ui, w2, "sheen", &params.sheen, setter);
                    srow(ui, w2, "sheen roughness", &params.sheen_rough, setter);
                    srow(ui, w2, "sheen tint", &params.sheen_tint, setter);
                    crow(ui, "sheen overlay", &params.sheen_overlay, setter);
                    help(ui, "Clearcoat = a thin smooth lacquer over the base (car paint, \
                             ceramic, wet) — a second glossy reflection; roughness makes it \
                             satin. Velvet/Sheen = a soft fuzz that lights up at grazing \
                             angles (velvet, dust, moss); tint 0 = white fuzz, up = the fuzz \
                             takes the node's own colour. Pick Clearcoat / Velvet as the \
                             material type for the full effect, or tick an OVERLAY to add \
                             the lobe onto Standard/Chrome (lacquer a brushed metal, dust \
                             any surface). Glass/Refractive keep their transmissive look.");
                    ui.label(egui::RichText::new("— body optics (#214 T3) —").weak().small());
                    // Sheen shapers (mirrored from the Surface FX card so picking the
                    // Subsurface material puts the look controls right here). Same params
                    // as Surface FX → translucency/distortion/power — last-touched-wins.
                    srow(ui, w2, "translucency amount", &params.subsurface, setter);
                    srow(ui, w2, "translucency distortion", &params.sss_distortion, setter);
                    srow(ui, w2, "translucency power", &params.sss_power, setter);
                    srow(ui, w2, "translucency thickness", &params.sss_thickness, setter);
                    srow(ui, w2, "translucency radius", &params.sss_radius, setter);
                    srow(ui, w2, "interior scatter", &params.interior_scatter, setter);
                    help(ui, "Subsurface = honest wax/jade/marble. Shape the sheen with \
                             AMOUNT (strength — forced on for the Subsurface material), \
                             DISTORTION (wraps the glow around the surface — also widens the \
                             front-lit bleed), and POWER (tight rim ↔ broad wash). These \
                             mirror the Surface FX translucency sliders. THICKNESS + RADIUS \
                             then absorb the glow over each node's MEASURED thickness (thin \
                             edges glow, thick centres go deep) — now visible front-lit too, \
                             not only backlit; radius = how deep light travels before the \
                             node's colour absorbs it. 'interior scatter' lights up a \
                             Glass/Refractive body where it absorbs — opal / nebula-in-a-cube \
                             (needs some absorption; crystal-clear glass stays clear). \
                             Instanced cube/tube modes.");
                    ui.label(egui::RichText::new("— microstructure (#214 T4) —").weak().small());
                    srow(ui, w2, "glitter", &params.glitter, setter);
                    srow(ui, w2, "glitter density", &params.glitter_density, setter);
                    srow(ui, w2, "glitter sharpness", &params.glitter_sharpness, setter);
                    srow(ui, w2, "diffraction", &params.diffraction, setter);
                    srow(ui, w2, "diffraction freq", &params.diffraction_freq, setter);
                    srow(ui, w2, "retroreflection", &params.retro, setter);
                    help(ui, "Glitter = sparse sparkle flakes (metallic flake, frost) that \
                             twinkle as the light/camera move — density sets flake size, \
                             sharpness their tightness. Best with TAA on (they resolve like \
                             the stochastic glass; grainy without it). Diffraction = a grating \
                             rainbow on the reflection (CD / holographic foil — strongest over \
                             Chrome); frequency sets how many rainbow bands. Retroreflection = \
                             a glow straight back toward the light (road sign, cat's-eye), \
                             brightest when you look along the key light. All 0 = off. \
                             Standard/Chrome; instanced cube/tube modes.");
                    ui.label(egui::RichText::new("— spectral emission (#214 T5) —").weak().small());
                    srow(ui, w2, "fluorescence", &params.fluorescence, setter);
                    srow(ui, w2, "fluorescence hue", &params.fluor_hue, setter);
                    srow(ui, w2, "incandescence", &params.incandescence, setter);
                    srow(ui, w2, "temperature (K)", &params.temperature, setter);
                    help(ui, "Fluorescence = the surface soaks up the environment's blue/UV-ish \
                             light and glows in the chosen hue (blacklight-poster look — brightest \
                             under a blue or bright sky/HDR). Incandescence = a blackbody glow by \
                             temperature (≈1000K deep-red ember → 3000K warm → 6500K white → \
                             12000K blue) added on top of any material. Both 0 = off; they add \
                             into the emissive so they bloom with the HDR pipeline and apply on \
                             every material type.");
                    ui.label(egui::RichText::new("— spectral glass (#80) —").weak().small());
                    srow(ui, w2, "dispersion", &params.glass_dispersion, setter);
                    srow(ui, w2, "caustic", &params.glass_caustic, setter);
                    srow(ui, w2, "thin-film", &params.glass_thin_film, setter);
                    help(ui, "Spectral controls apply to the Glass type. Dispersion splits \
                             refraction into a rainbow at the edges (0 = today's glass); \
                             caustic brightens focused light through the body; thin-film \
                             adds an oil-slick sheen at grazing angles.");
                    ui.label(egui::RichText::new("— reflection look (#163) —").weak().small());
                    srow(ui, w2, "chrome purity", &params.chrome_purity, setter);
                    srow(ui, w2, "glass clarity", &params.glass_clarity, setter);
                    srow(ui, w2, "reflectivity (Std)", &params.f0_override, setter);
                    srow(ui, w2, "reflect palette", &params.reflect_tint, setter);
                    ui.label(egui::RichText::new("— live-sky clouds (#305 T2) —").weak().small());
                    crow(ui, "reflect drifting clouds", &params.sky_reflect_clouds, setter);
                    srow(ui, w2, "cloud cover", &params.sky_cloud_cover, setter);
                    srow(ui, w2, "cloud speed /beat", &params.sky_cloud_speed, setter);
                    srow(ui, w2, "cloud strength", &params.sky_cloud_strength, setter);
                    help(ui, "Drifting procedural clouds on the SHARP environment reflection \
                             (chrome / clear glass / beads), so mirrors show a moving sky \
                             instead of a frozen one. A cheap approximation layered on the \
                             env reflection — off = today's reflection, byte-identical.");
                    help(ui, "All 0 = today's look. Chrome purity → a pure NEUTRAL mirror \
                             (sharp, untinted); glass clarity → colourless CLEAR glass; \
                             reflectivity lifts Standard toward a mirror without metallic=1. \
                             Reflect palette tints the reflection by the cube's colour \
                             (0 = neutral, >1 = override). For cube-to-cube mirroring, also \
                             enable Reflections (SSR) →.");
                });
                card(&mut c[1], "Reflections (SSR)", |ui| {
                    crow(ui, "enable (screen-space)", &params.ssr, setter);
                    srow(ui, w2, "intensity", &params.ssr_intensity, setter);
                    srow(ui, w2, "max roughness", &params.ssr_max_roughness, setter);
                    srow(ui, w2, "thickness", &params.ssr_thickness, setter);
                    srow(ui, w2, "steps", &params.ssr_steps, setter);
                    help(ui, "Off by default. Screen-space: cubes reflect each other (a hall \
                             of mirrors on Chrome) AND the rest of the on-screen world — \
                             terrain, particles, membrane, the Z0NE corridor flying past — \
                             which RT reflections (TLAS = cubes only) can't see. Enable it \
                             ALONGSIDE RT reflections for a hybrid: RT wins on the cubes \
                             (off-screen too), SSR fills in everything else on screen; a miss \
                             falls back to the environment with no seam. Screen-space, so \
                             off-screen content dropping out is expected. Keep max-roughness \
                             low so it stays off the diffuse cubes; steps is the perf dial.");
                    ui.label(egui::RichText::new("— parallax probe (#163) —").weak().small());
                    param_combo(ui, w2, "source", &params.refl_source, setter);
                    srow(ui, w2, "box scale", &params.refl_box_scale, setter);
                    srow(ui, w2, "box height", &params.refl_box_height, setter);
                    srow(ui, w2, "parallax blend", &params.refl_blend, setter);
                    help(ui, "Env Only = today (reflection depends only on face angle — the \
                             \"painted-on sky\" look). Parallax Box intersects the reflection \
                             against the field's bounding box so it also shifts with a cube's \
                             POSITION — reflections gain depth. Box scale/height fit the box to \
                             the structure; blend fades between infinite and box-projected. \
                             Reuses the env map (no extra passes); pair with SSR for cube-to-cube.");
                });
                card(&mut c[1], "Global Illumination", |ui| {
                    crow(ui, "enable (bounced GI)", &params.gi, setter);
                    srow(ui, w2, "intensity", &params.gi_intensity, setter);
                    srow(ui, w2, "reach", &params.gi_falloff, setter);
                    help(ui, "Off by default. A coarse probe volume bleeds coloured light from \
                             cube to cube — a bright/coloured strand tints its neighbours. \
                             Strongest with a palette or Swept-Tubes colour sweep (which give \
                             the nodes real colour to bounce).");
                    ui.label(egui::RichText::new("— cubes as lights (#167 T3) —").weak().small());
                    crow(ui, "enable (real lights)", &params.ml_enabled, setter);
                    srow(ui, w2, "intensity", &params.ml_intensity, setter);
                    srow(ui, w2, "radius", &params.ml_radius, setter);
                    srow(ui, w2, "count", &params.ml_count, setter);
                    crow(ui, "ReSTIR (Tier 5d)", &params.ml_restir, setter);
                    help(ui, "Off by default. The brightest cubes become REAL point lights: a \
                             glowing cube throws a crisp specular glint + a coloured diffuse \
                             pool onto its neighbours (Cook-Torrance, per-fragment) — the direct \
                             lighting that bloom + GI only fake. Count = how many to use (perf); \
                             radius scales the scene size. Instanced path only. RESTIR (Tier 5d) \
                             picks the COUNT lights by weighted reservoir sampling instead of a \
                             hard brightest-N cap: every glowing cube gets a luminance-proportional \
                             chance, so dim / distant / off-screen emitters rotate into the set \
                             over time (the light fade + TAA integrate it) — a 50k-node field lit \
                             by all its cubes, not just the top COUNT. Per-light RT shadowing is a \
                             follow-up.");
                });
                card(&mut c[1], "Screen-Space GI (#152)", |ui| {
                    crow(ui, "enable (SSGI)", &params.ssgi, setter);
                    srow(ui, w2, "intensity", &params.ssgi_intensity, setter);
                    srow(ui, w2, "radius", &params.ssgi_radius, setter);
                    srow(ui, w2, "rays", &params.ssgi_rays, setter);
                    help(ui, "Off by default. One screen-space diffuse bounce — bright cubes \
                             bleed colour onto neighbours. Noisy at low ray counts; pair with \
                             TAA (below) to clean it up. Instanced/node-field paths only.");
                });
                card(&mut c[1], "Voxel GI (#152)", |ui| {
                    crow(ui, "enable (VXGI)", &params.vxgi_enabled, setter);
                    srow(ui, w2, "intensity", &params.vxgi_intensity, setter);
                    srow(ui, w2, "rays", &params.vxgi_rays, setter);
                    srow(ui, w2, "steps", &params.vxgi_steps, setter);
                    help(ui, "Off by default. Voxelizes the node field + marches it in world \
                             space, so bright/emissive cubes bleed colour onto neighbours — \
                             including off-screen/occluded ones (unlike SSGI). Adds a \
                             volumetric bounce that blooms. Noisy at low rays; pair with TAA. \
                             Instanced/node-field paths only; heavier than SSGI.");
                    ui.label(egui::RichText::new("— specular reflections (#163 T3) —").weak().small());
                    srow(ui, w2, "reflection", &params.vxgi_spec_strength, setter);
                    srow(ui, w2, "aperture", &params.vxgi_spec_aperture, setter);
                    srow(ui, w2, "reach", &params.vxgi_spec_reach, setter);
                    srow(ui, w2, "refl steps", &params.vxgi_spec_steps, setter);
                    help(ui, "Reflection strength 0 = off. Cone-traces the SAME voxel volume \
                             along the reflection ray, so cubes reflect the ACTUAL scene — \
                             other cubes, off-screen emitters — with no screen-edge dropout \
                             (unlike SSR). Requires VXGI enabled (above). Aperture widens the \
                             cone (0 = sharp, 1 = glossy blur); reach scales the march by the \
                             scene size; refl steps is the perf dial.");
                });
                card(&mut c[1], "Ray Tracing — Hardware (#195)", |ui| {
                    if !rt_available {
                        ui.label(
                            egui::RichText::new(
                                "✕ no hardware ray tracing on this GPU/backend",
                            )
                            .weak()
                            .small(),
                        );
                    }
                    ui.add_enabled_ui(rt_available, |ui| {
                        crow(ui, "enable (build BLAS/TLAS)", &params.rt_enable, setter);
                        // Live again on wgpu 30 — 29's Metal ray-query
                        // dispatch wedged the GPU machine-wide (#195).
                        param_combo(ui, w2, "debug view", &params.rt_debug, setter);
                        ui.label(egui::RichText::new("— shadows (Tier 1) —").weak().small());
                        crow(ui, "RT shadows (key)", &params.rt_shadows, setter);
                        srow(ui, w2, "softness", &params.rt_shadow_soft, setter);
                        srow(ui, w2, "strength", &params.rt_shadow_strength, setter);
                        crow(ui, "fill shadow", &params.rt_shadow_fill, setter);
                        ui.label(egui::RichText::new("— reflections (Tier 2) —").weak().small());
                        crow(ui, "RT reflections", &params.rt_reflect, setter);
                        srow(ui, w2, "intensity", &params.rt_reflect_intensity, setter);
                        srow(ui, w2, "max roughness", &params.rt_reflect_rough, setter);
                        srow(ui, w2, "reach", &params.rt_reflect_reach, setter);
                        srow(ui, w2, "rays", &params.rt_reflect_rays, setter);
                        crow(ui, "hit shadows", &params.rt_reflect_shadows, setter);
                        ui.label(egui::RichText::new("— global illumination (Tier 4) —").weak().small());
                        crow(ui, "RT GI (one bounce)", &params.rt_gi, setter);
                        srow(ui, w2, "intensity", &params.rt_gi_intensity, setter);
                        srow(ui, w2, "rays", &params.rt_gi_rays, setter);
                        srow(ui, w2, "reach", &params.rt_gi_reach, setter);
                        crow(ui, "hit shadows", &params.rt_gi_shadows, setter);
                        ui.label(egui::RichText::new("— temporal denoise (Tier 4½) —").weak().small());
                        crow(ui, "temporal accumulate", &params.rt_temporal, setter);
                        srow(ui, w2, "feedback", &params.rt_temporal_feedback, setter);
                        srow(ui, w2, "beat relax", &params.rt_temporal_beat, setter);
                        crow(ui, "variance (SVGF)", &params.rt_temporal_variance, setter);
                        srow(ui, w2, "max samples", &params.rt_temporal_accum, setter);
                        srow(ui, w2, "clamp width", &params.rt_temporal_clamp, setter);
                        ui.label(egui::RichText::new("— denoise (Tier 4½) —").weak().small());
                        crow(ui, "RT denoise", &params.rt_denoise, setter);
                        srow(ui, w2, "amount", &params.rt_denoise_amount, setter);
                        ui.label(egui::RichText::new("— neural denoise (Tier 5a) —").weak().small());
                        crow(ui, "neural denoise", &params.nd_enable, setter);
                        srow(ui, w2, "net strength", &params.nd_strength, setter);
                        srow(ui, w2, "seed", &params.nd_seed, setter);
                        srow(ui, w2, "feature scale", &params.nd_omega, setter);
                        if params.nd_enable.value() {
                            let n = params.nd_strength.value();
                            let msg = if !params.rt_denoise.value() {
                                "neural denoise: idle — enable RT denoise above".to_string()
                            } else if n <= 0.0 {
                                "neural denoise: ON — net 0 (≡ classical à-trous)".to_string()
                            } else {
                                format!("neural denoise: ON — net {n:.2} (learned kernel)")
                            };
                            ui.label(egui::RichText::new(msg).weak().small());
                        }
                    });
                    if rt_available && params.rt_enable.value() {
                        ui.label(
                            egui::RichText::new(format!(
                                "TLAS rebuild: {rt_tlas_ms:.2} ms/frame"
                            ))
                            .weak()
                            .small(),
                        );
                    }
                    if rt_available {
                        // Path tracer (#200 Tier 4): per-display toggle, edge-detected
                        // by the visual so this checkbox and the 'P' key agree
                        // (last-touched-wins). The line below reports the live state.
                        crow(ui, "path tracer (ground truth)", &params.pathtrace_enable, setter);
                        ui.label(
                            egui::RichText::new(if pathtrace_active {
                                format!(
                                    "path tracer: ON — {pathtrace_spp} spp (pause Speed to converge)"
                                )
                            } else {
                                "path tracer: off — progressive ground-truth reference (also 'P' in the visual)".to_string()
                            })
                            .weak()
                            .small(),
                        );
                        crow(ui, "PT dielectric glass (#258 T2)", &params.pt_dielectric, setter);
                        srow(ui, w2, "PT absorption", &params.pt_absorb, setter);
                        help(ui, "Path-traced dielectric glass (#258 Tier 2): with the path \
                                 tracer ON, Glass/Refractive nodes refract through BOTH \
                                 surfaces — real two-surface transmission (the object behind \
                                 seen through the glass; glass-through-glass), with Fresnel \
                                 reflect/transmit, total-internal-reflection, and Beer–Lambert \
                                 absorption through the body. Absorption darkens/tints thick \
                                 bodies (0 = clear glass). Needs Material = Glass or Refractive, \
                                 and converges over a few seconds on a STILL camera (pause \
                                 Speed). Off = the diffuse-only path tracer, unchanged.");
                        param_combo(ui, w2, "PT composite", &params.pt_composite, setter);
                        srow(ui, w2, "PT augment", &params.pt_augment, setter);
                        crow(ui, "spectral dispersion (#258 T4)", &params.spectral_enable, setter);
                        srow(ui, w2, "Abbe number", &params.spectral_abbe, setter);
                        srow(ui, w2, "spectral samples", &params.spectral_secondaries, setter);
                        help(ui, "Spectral dispersion (#258 Tier 4): each traced path carries \
                                 ONE wavelength and glass / the Lens refracts at a per-wavelength \
                                 Cauchy IOR — a prism or dispersive lens throws a REAL rainbow \
                                 that refracts correctly through the next glass body. Needs the \
                                 path tracer + Material = Glass (or the Lens). Abbe number = \
                                 dispersion strength: LOW (~25) = wide rainbow (flint glass), HIGH \
                                 (~80) = subtle; spectral samples trade colour noise for cost. Off \
                                 = the RGB tracer, unchanged. Converges over a few still frames.");
                        crow(ui, "PT caustics (#258 T5)", &params.pt_caustics, setter);
                        srow(ui, w2, "caustic photons ×1k", &params.pt_caustic_photons, setter);
                        srow(ui, w2, "caustic intensity", &params.pt_caustic_intensity, setter);
                        srow(ui, w2, "caustic radius", &params.pt_caustic_radius, setter);
                        help(ui, "Photon-mapped caustics (#258 Tier 5): a light-tracing pass \
                                 fires photons from the key light through the glass / chrome / \
                                 Lens specular chain each frame and splats where they land — so \
                                 the FOCUSED light a lens or prism casts ON a surface (the focal \
                                 hot-spot, the rainbow on the floor, glass concentrating the key \
                                 light) appears in about a frame instead of converging over \
                                 thousands. Disperses per-wavelength when spectral is on. Needs \
                                 the path tracer + something specular. Photons = budget per frame \
                                 (cost/smoothness); radius = the screen-space gather blur. Off = \
                                 the tracer unchanged.");
                        help(ui, "How the path tracer reaches the frame. REPLACE (default) \
                                 overwrites the image with the trace — ground truth, but you \
                                 lose the environment + raster PBR facilities. BLEND keeps the \
                                 full raster PBR render and cross-blends the trace over it by \
                                 'PT augment' (environment + materials stay; the trace's \
                                 accurate refraction/GI layers on). GI ADD has the tracer \
                                 contribute INDIRECT light only (no double-counted direct), \
                                 added onto the raster — physically-clean augmentation. Augment \
                                 = the blend opacity / GI gain (0 = raster untouched).");
                        // Neural radiance cache — live (#256 Tier 0).
                        crow(ui, "radiance cache (#256 T0)", &params.nrc_enable, setter);
                        ui.label(
                            egui::RichText::new(match nrc_state {
                                0 => "radiance cache: off".to_string(),
                                2 => format!("radiance cache: CONVERGED — train loss {nrc_loss:.4}"),
                                _ => format!("radiance cache: warming — train loss {nrc_loss:.4}"),
                            })
                            .weak()
                            .small(),
                        );
                        srow(ui, w2, "cache confidence", &params.nrc_confidence, setter);
                        srow(ui, w2, "cache learn rate", &params.nrc_learn_rate, setter);
                        srow(ui, w2, "cache frequency", &params.nrc_omega, setter);
                        srow(ui, w2, "cache terminate bounce", &params.nrc_terminate, setter);
                        srow(ui, w2, "cache train samples", &params.nrc_train_samples, setter);
                        srow(ui, w2, "cache seed", &params.nrc_seed, setter);
                        // Cache RT-stack synergies (#256 Tier 1).
                        crow(ui, "cache-guided sampling (#256 T1)", &params.nrc_guide, setter);
                        srow(ui, w2, "guide candidates", &params.nrc_guide_candidates, setter);
                        crow(ui, "cache firefly clamp (#256 T1)", &params.nrc_firefly, setter);
                        srow(ui, w2, "firefly clamp strength", &params.nrc_firefly_clamp, setter);
                        help(ui, "Cache RT-stack synergies (#256 Tier 1) — both use the live \
                                 radiance cache to make the path tracer CHEAPER, and both need \
                                 the cache ON. GUIDED SAMPLING chooses each bounce by importance-\
                                 sampling the cache (paths follow the light instead of scattering \
                                 blindly → the image converges faster for the same quality). \
                                 FIREFLY CLAMP caps each sample toward the cache mean (the cache \
                                 is the expected value), killing bright single-sample sparkles \
                                 before the denoiser. Off = the tracer unchanged.");
                        // Cache light-field uses (#256 Tier 2).
                        crow(ui, "cache GI — supersede DDGI (#256 T2)", &params.nrc_gi, setter);
                        srow(ui, w2, "cache GI strength", &params.nrc_gi_strength, setter);
                        crow(ui, "cache-lit reflections (#256 T2)", &params.nrc_reflect, setter);
                        help(ui, "Cache light-field uses (#256 Tier 2) — because the cache \
                                 knows the radiance everywhere, it can light more than the \
                                 path-traced cubes (needs the cache ON). CACHE GI fills the \
                                 bounced-GI probe volume from the continuous cache instead of \
                                 the discrete grid — a learned, continuous bounce field that \
                                 also lights the ink / fluid (raster path; works without the \
                                 path tracer). CACHE-LIT REFLECTIONS makes Chrome/Glass \
                                 reflections in the path tracer show the LIT neighbours + \
                                 off-screen light, not just the environment map (needs the \
                                 path tracer + PT dielectric glass). Off = the discrete probe \
                                 grid + env-only reflections, unchanged.");
                        // Cache hard transport + volumetrics (#256 Tier 3).
                        crow(ui, "cache volumetrics (#256 T3)", &params.nrc_volume, setter);
                        srow(ui, w2, "volumetric density", &params.nrc_volume_density, setter);
                        srow(ui, w2, "volumetric steps", &params.nrc_volume_steps, setter);
                        srow(ui, w2, "volumetric strength", &params.nrc_volume_strength, setter);
                        crow(ui, "cached caustics (#256 T3)", &params.nrc_caustic, setter);
                        srow(ui, w2, "caustic gain", &params.nrc_caustic_gain, setter);
                        help(ui, "Cache hard transport (#256 Tier 3) — the cache amortizes the \
                                 rare / expensive light paths (needs the path tracer + the cache \
                                 ON). CACHE VOLUMETRICS marches the camera ray through a hazy \
                                 medium and lights each step from the cache → god-rays / \
                                 atmospheric glow that pulses with the music (feeds bloom). \
                                 Density = haze thickness, steps = smoothness, strength = glow. \
                                 CACHED CAUSTICS adds the focused light that concentrates through \
                                 glass — the bright pools a lens or prism casts — read from the \
                                 cache and bloomed. Off = the tracer unchanged.");
                        help(ui, "Neural radiance cache (#256 Tier 0): with the path tracer \
                                 ON, the visual trains a tiny SIREN network of the scene's light \
                                 field each frame; short paths TERMINATE into a cache query at \
                                 'terminate bounce' instead of tracing on — infinite-bounce GI at \
                                 short-path cost, so a still frame converges faster and cleaner. \
                                 'confidence' blends the cache against the raw trace (a cold / \
                                 wrong cache can only lose GI, never corrupt the image — the raw \
                                 trace is the fallback). The bake-first target is the environment \
                                 light field; per-pixel bounced-GI online training is an on-Mac \
                                 follow-up. Off = the tracer unchanged.");
                    }
                    help(ui, "Hardware ray tracing (M3+ Macs; greyed out elsewhere; \
                             instanced paths only). Enable builds the acceleration structure \
                             (BLAS/TLAS) over the field each frame — the readout is its \
                             per-frame cost; the debug view overlays a fullscreen ray query \
                             to verify it (per-display, not saved in presets). RT SHADOWS \
                             trace one ray per pixel at the key light instead of sampling \
                             the 2048² shadow map: ground-truth contact shadows, no \
                             bias/acne tuning, and (unlike the map) an optional second ray \
                             shadows the FILL light. Softness is the light's angular size — \
                             pair with TAA to resolve the penumbra; strength is how dark \
                             occlusion gets. RT REFLECTIONS trace the mirrored view ray \
                             against the actual scene — neighbours, off-screen and \
                             behind-camera cubes appear in mirrors (SSR can't) — and \
                             supersede SSR while on; a miss falls back to the environment \
                             with no seam. Max roughness is the cutoff above which the \
                             env look stands; hit shadows lets reflections contain \
                             shadows (one extra ray). RT GI gathers one indirect bounce \
                             per pixel — real inter-cube colour bleed including \
                             off-screen emitters (SSGI only sees on-screen); it \
                             supersedes the SSGI march while on, and a miss leaves the \
                             scene's own IBL ambient. Rays is the per-pixel gather count \
                             (pair with TAA); reach is how far indirect light travels. \
                             TEMPORAL ACCUMULATE denoises the RT reflection + GI buffers \
                             by reprojecting the previous frame's result by camera motion \
                             and blending it in (an exponential moving average), so the \
                             low-sample grain integrates out over time; a 3×3 clamp \
                             rejects stale history where geometry moved. Feedback is the \
                             history weight (higher = smoother, more lag); beat relax \
                             drops that weight on each beat kick so history doesn't smear \
                             across the fast auto-orbit camera. VARIANCE (SVGF) upgrades \
                             it to true SVGF: fresh pixels converge faster (history \
                             weight ramps with an accumulated-sample count up to MAX \
                             SAMPLES) and history luma is rejected by a σ-clamp of width \
                             CLAMP WIDTH (μ ± γσ) instead of a raw min/max box, so a \
                             single firefly stops swelling the clamp — lower width = \
                             crisper/noisier, higher = softer/smoother. \
                             RT DENOISE runs an edge-aware à-trous filter over the traced \
                             reflection + GI buffers before compositing — cleans the \
                             low-sample grain without crossing depth/highlight edges; \
                             reflections are filtered roughness-adaptively (sharp mirrors \
                             stay crisp). Off = raw jitter. NEURAL DENOISE (Tier 5a) \
                             swaps that à-trous for a kernel-predicting filter: a tiny \
                             seeded MLP reshapes the bilateral kernel per pixel from the \
                             local edge features — the neural rung on top of the \
                             classical one. NET STRENGTH is the network's influence; at \
                             0 it reproduces the classical filter byte-for-byte (off = \
                             classical), SEED swaps the learned kernel, FEATURE SCALE is \
                             the network frequency. All effects imply the TLAS build.");
                });
                // (The old "Neural Field (#200 Tier 0 — foundation)" Look card was
                // removed once Tier 1 shipped: seed A/B, latent walk and feature
                // scale all live in the Neural Field GENERATOR card now, so the Look
                // duplicate only caused confusion. `neural_enable` (neural[0]) stays
                // in the params (inert, preset-captured) but no longer has a row.)
                card(&mut c[2], "Temporal — TAA / Motion Blur (#152)", |ui| {
                    crow(ui, "TAA (anti-alias)", &params.taa_enabled, setter);
                    srow(ui, w2, "TAA blend", &params.taa_blend, setter);
                    srow(ui, w2, "TAA sharpen", &params.taa_sharpen, setter);
                    crow(ui, "motion blur", &params.motion_blur, setter);
                    srow(ui, w2, "MB amount", &params.mb_amount, setter);
                    srow(ui, w2, "MB samples", &params.mb_samples, setter);
                    crow(ui, "stochastic glass (OIT)", &params.stochastic_glass, setter);
                    help(ui, "Off by default (per-display, not preset-captured). TAA reprojects \
                             + accumulates history to anti-alias & stabilise; lower blend = more \
                             history (smoother, more ghosting). Motion blur uses the camera \
                             velocity. Stochastic glass = order-independent transparency for \
                             stacked Glass (needs TAA on to resolve the dither). Velocity is \
                             camera-only on node-field paths.");
                });
                card(&mut c[0], "Surface FX", |ui| {
                    srow(ui, w2, "translucency", &params.subsurface, setter);
                    srow(ui, w2, "  distortion", &params.sss_distortion, setter);
                    srow(ui, w2, "  power", &params.sss_power, setter);
                    srow(ui, w2, "iridescence", &params.iridescence, setter);
                    srow(ui, w2, "  scale", &params.irid_scale, setter);
                    srow(ui, w2, "  hue", &params.irid_shift, setter);
                });
                card(&mut c[1], "Bioluminescence", |ui| {
                    srow(ui, w2, "colour cycle", &params.color_cycle, setter);
                    srow(ui, w2, "ripple intensity", &params.ripple_intensity, setter);
                    srow(ui, w2, "  speed", &params.ripple_speed, setter);
                    srow(ui, w2, "  frequency", &params.ripple_freq, setter);
                    srow(ui, w2, "  sharpness", &params.ripple_sharp, setter);
                    param_combo(ui, w2, "  geometry", &params.ripple_geom, setter);
                    help(ui, "Colour cycle flows the palette along the sweep. Ripple sends \
                             a travelling HDR emissive pulse through the field (push \
                             intensity > 1 to bloom). Both free-run.");
                });
                card(&mut c[1], "Reaction-Diffusion Skin", |ui| {
                    srow(ui, w2, "intensity", &params.rd_intensity, setter);
                    srow(ui, w2, "feed", &params.rd_feed, setter);
                    srow(ui, w2, "kill", &params.rd_kill, setter);
                    srow(ui, w2, "scale", &params.rd_scale, setter);
                    srow(ui, w2, "pigment", &params.rd_albedo_mix, setter);
                    help(ui, "Turing pattern crawling over the surface (any mode). \
                             Intensity = HDR glow (0 = off); feed/kill morph \
                             spots ↔ stripes ↔ maze; pigment carves albedo.");
                });
                } // end KIFS-hidden look cards (Cast Shadows → reaction-diffusion)
                card(&mut c[0], "Lighting (Direct)", |ui| {
                    srow(ui, w2, "ambient", &params.ambient, setter);
                    srow(ui, w2, "key", &params.key_intensity, setter);
                    srow(ui, w2, "fill", &params.fill_intensity, setter);
                    srow(ui, w2, "elevation", &params.elevation, setter);
                    srow(ui, w2, "azimuth", &params.azimuth, setter);
                });
                card(&mut c[0], "Environment (IBL)", |ui| {
                    srow(ui, w2, "exposure", &params.exposure, setter);
                    srow(ui, w2, "intensity", &params.env_intensity, setter);
                    srow(ui, w2, "rotation", &params.env_rotation, setter);
                    crow(ui, "show background", &params.bg_visible, setter);
                    param_combo(ui, w2, "bg tone map", &params.bg_tonemap, setter);
                    srow(ui, w2, "bg bright", &params.bg_intensity, setter);
                    srow(ui, w2, "tint hue", &params.env_tint_hue, setter);
                    srow(ui, w2, "tint amount", &params.env_tint_amt, setter);
                });
                card(&mut c[2], "Particle Aura", |ui| {
                    param_combo(ui, w2, "tier", &params.particles_tier, setter);
                    srow(ui, w2, "speed", &params.particles_speed, setter);
                    srow(ui, w2, "lifetime", &params.particles_lifetime, setter);
                    srow(ui, w2, "spawn radius", &params.particles_spawn_radius, setter);
                    srow(ui, w2, "drag", &params.particles_drag, setter);
                    srow(ui, w2, "turbulence", &params.particles_turbulence, setter);
                    ui.label(egui::RichText::new("— look —").weak().small());
                    srow(ui, w2, "size", &params.particles_size, setter);
                    srow(ui, w2, "emissive (HDR)", &params.particles_emissive, setter);
                    srow(ui, w2, "opacity", &params.particles_alpha, setter);
                    srow(ui, w2, "palette hue", &params.particles_hue_shift, setter);
                    ui.label(egui::RichText::new("— shaded beads (#298) —").weak().small());
                    crow(ui, "beads (IBL droplets)", &params.particles_beads, setter);
                    srow(ui, w2, "bead metallic", &params.particles_metallic, setter);
                    srow(ui, w2, "bead roughness", &params.particles_roughness, setter);
                    param_combo(ui, w2, "bead material", &params.particles_material, setter);
                    param_combo(ui, w2, "bead shape", &params.particles_shape, setter);
                    srow(ui, w2, "bead IOR (glass)", &params.particles_ior, setter);
                    srow(ui, w2, "bead shape amount", &params.particles_shape_param, setter);
                    crow(ui, "beads in RT (needs RT master)", &params.particles_beads_rt, setter);
                    srow(ui, w2, "bead hue", &params.particles_bead_hue, setter);
                    srow(ui, w2, "bead hue cycle /beat", &params.particles_bead_hue_cycle, setter);
                    srow(ui, w2, "bead saturation", &params.particles_bead_sat, setter);
                    srow(ui, w2, "bead value", &params.particles_bead_val, setter);
                    srow(ui, w2, "bead emissive (HDR)", &params.particles_bead_emissive, setter);
                    crow(ui, "hide generator (aura / ink only)", &params.particles_hide_generator, setter);
                    crow(ui, "ribbons (motion blur)", &params.particles_ribbon, setter);
                    srow(ui, w2, "ribbon stretch", &params.particles_ribbon_stretch, setter);
                    srow(ui, w2, "beat burst", &params.particles_beat_burst, setter);
                    ui.label(egui::RichText::new("— fluid (tier = Fluid) —").weak().small());
                    srow(ui, w2, "stir force", &params.fluid_force, setter);
                    srow(ui, w2, "vorticity (eddies)", &params.fluid_vorticity, setter);
                    srow(ui, w2, "dissipation", &params.fluid_dissipation, setter);
                    srow(ui, w2, "inflow decay", &params.fluid_inflow_decay, setter);
                    srow(ui, w2, "pressure iters", &params.fluid_iters, setter);
                    ui.label(egui::RichText::new("— performance —").weak().small());
                    srow(ui, w2, "count (×1000)", &params.particles_count_k, setter);
                    srow(ui, w2, "grid resolution", &params.particles_grid_res, setter);
                });
                card(&mut c[2], "Fluid Ink (#182)", |ui| {
                    crow(ui, "enabled", &params.ink_enabled, setter);
                    // The same param as the Particle Aura's checkbox
                    // (one flag, surfaced in both cards — it already
                    // governs the ink's pure-medium view).
                    crow(ui, "hide generator (medium only)", &params.particles_hide_generator, setter);
                    srow(ui, w2, "injection rate", &params.ink_rate, setter);
                    srow(ui, w2, "injection radius", &params.ink_radius, setter);
                    ui.label(egui::RichText::new("— look —").weak().small());
                    srow(ui, w2, "extinction", &params.ink_extinction, setter);
                    srow(ui, w2, "scatter", &params.ink_scatter, setter);
                    srow(ui, w2, "emissive (HDR)", &params.ink_emissive, setter);
                    srow(ui, w2, "anisotropy g", &params.ink_anisotropy, setter);
                    srow(ui, w2, "dissipation", &params.ink_dissipation, setter);
                    srow(ui, w2, "reveal (cull haze)", &params.ink_reveal, setter);
                    srow(ui, w2, "micro-detail", &params.fl2_detail, setter);
                    ui.label(egui::RichText::new("— medium (T2) —").weak().small());
                    crow(ui, "solid boundaries (no-slip)", &params.fl2_boundaries, setter);
                    srow(ui, w2, "buoyancy (±)", &params.fl2_buoyancy, setter);
                    srow(ui, w2, "heat decay", &params.fl2_heat_decay, setter);
                    srow(ui, w2, "beat splash", &params.fl2_splash, setter);
                    srow(ui, w2, "beat dye gate", &params.fl2_dye_gate, setter);
                    ui.label(egui::RichText::new("— quality / performance —").weak().small());
                    crow(ui, "sharp advection (MacCormack)", &params.ink_maccormack, setter);
                    crow(ui, "half-res march", &params.ink_half_res, setter);
                    srow(ui, w2, "march steps", &params.ink_steps, setter);
                    srow(ui, w2, "sim res override", &params.fl2_res, setter);
                    srow(ui, w2, "sim substeps", &params.fl2_substeps, setter);
                    help(ui, "The generator stirs a fluid; this renders the medium \
                             itself: an RGB dye injected at the nodes (their live \
                             colours), advected by the Navier-Stokes solve, and \
                             raymarched as a lit volume (key-light scatter + IBL \
                             ambient + emissive glow). Runs the fluid solver even \
                             when the Particle Aura tier isn't Fluid — tune the stir \
                             with the aura card's fluid dials. Pair with 'hide \
                             generator' for pure ink. Reveal culls the dilute haze \
                             (like the vector-field reveal) so the dense filaments \
                             inside show through. Node generators only. \
                             Medium (T2): solid boundaries make the geometry real \
                             no-slip walls (wakes, channeled flow); buoyancy makes \
                             hot fresh ink rise (+) or sink (-); micro-detail adds \
                             vorticity-scaled curl swirl at render time; beat \
                             splash kicks the medium radially on the pulse and the \
                             dye gate puffs ink on the beat. Sim res override 0 = \
                             the aura grid dial (128 max is heavy); substeps \
                             stabilise fast stirs at full solver cost each.");
                });
                card(&mut c[2], "Liquid (#182 T3)", |ui| {
                    crow(ui, "enabled", &params.liq_enabled, setter);
                    // The same shared flag as the Particle Aura /
                    // Fluid Ink cards — one param, surfaced wherever
                    // a pure-medium view makes sense.
                    crow(ui, "hide generator (liquid only)", &params.particles_hide_generator, setter);
                    crow(ui, "hidden generator keeps lighting", &params.ghost_light, setter);
                    if ui
                        .button("Reset pool")
                        .on_hover_text(
                            "Re-pour the liquid from its seed distribution \
                             (e.g. after gravity has drained it to the floor)",
                        )
                        .clicked()
                    {
                        liq_reset_gen.fetch_add(1, Ordering::Relaxed);
                    }
                    crow(ui, "generator collides (churn)", &params.liq_collide, setter);
                    srow(ui, w2, "stir gain", &params.liq_stir, setter);
                    ui.label(egui::RichText::new("— feel —").weak().small());
                    srow(ui, w2, "gravity", &params.liq_gravity, setter);
                    srow(ui, w2, "stiffness", &params.liq_stiffness, setter);
                    srow(ui, w2, "viscosity", &params.liq_viscosity, setter);
                    ui.label(egui::RichText::new("— tank —").weak().small());
                    param_combo(ui, w2, "container shape", &params.liq_shape, setter);
                    srow(ui, w2, "container size", &params.liq_container, setter);
                    srow(ui, w2, "vertical offset", &params.liq_offset_y, setter);
                    crow(ui, "open top (splash out)", &params.liq_open_top, setter);
                    srow(ui, w2, "reveal (soft window)", &params.liq_reveal, setter);
                    ui.label(egui::RichText::new("— surface —").weak().small());
                    srow(ui, w2, "density", &params.liq_density, setter);
                    srow(ui, w2, "threshold", &params.liq_threshold, setter);
                    srow(ui, w2, "hue", &params.liq_hue, setter);
                    srow(ui, w2, "saturation", &params.liq_sat, setter);
                    ui.label(egui::RichText::new("— performance —").weak().small());
                    srow(ui, w2, "particles (k)", &params.liq_count, setter);
                    srow(ui, w2, "grid resolution", &params.liq_res, setter);
                    srow(ui, w2, "substeps", &params.liq_substeps, setter);
                    help(ui, "An MLS-MPM particle liquid in an invisible tank \
                             centred on the field. Shapes: Sphere pools into a \
                             curved bowl, Boundless has NO wall — a soft shell \
                             absorbs strays and the liquid trails off into \
                             space; add Reveal to window the render spherically \
                             so no tank face ever shows. Gravity (default 0 = \
                             weightless; dial it up to pool) pulls it down, the \
                             generator's nodes churn it as moving obstacles, and \
                             the surface renders through the metaball isosurface \
                             with the full material stack — set Material = GLASS \
                             for water (IOR/reflections/refraction apply). Pair \
                             with 'hide generator' for a pure pool. Count/grid \
                             reseed the pool on change; substeps stabilise fast \
                             stirring at full solver cost each.");
                });
                card(&mut c[2], "Liquid Material", |ui| {
                    param_combo(ui, w2, "material", &params.liq_material, setter);
                    param_combo(ui, w2, "render", &params.liq_render, setter);
                    srow(ui, w2, "metallic", &params.liq_metallic, setter);
                    srow(ui, w2, "roughness", &params.liq_roughness, setter);
                    srow(ui, w2, "glow (HDR)", &params.liq_glow, setter);
                    srow(ui, w2, "IOR", &params.liq_ior, setter);
                    srow(ui, w2, "absorption", &params.liq_absorb, setter);
                    ui.label(egui::RichText::new("— reflections —").weak().small());
                    srow(ui, w2, "chrome purity", &params.liq_chrome_purity, setter);
                    srow(ui, w2, "glass clarity", &params.liq_glass_clarity, setter);
                    srow(ui, w2, "F0 override", &params.liq_f0, setter);
                    ui.label(egui::RichText::new("— spectral —").weak().small());
                    srow(ui, w2, "dispersion", &params.liq_dispersion, setter);
                    srow(ui, w2, "glass caustic", &params.liq_gcaustic, setter);
                    srow(ui, w2, "thin film", &params.liq_thin_film, setter);
                    help(ui, "The liquid's OWN material — a full, separate \
                             material card ('Use Scene Material' follows the \
                             scene selector; anything else overrides every dial \
                             here for the liquid only). Render: Isosurface = the \
                             classic metaball surface; Refractive = optically \
                             correct see-through water — Snell refraction of the \
                             actual scene at the IOR, real thickness, \
                             Beer–Lambert absorption (the liquid hue tints the \
                             depths), energy-conserving Fresnel. Absorption \
                             deepens the colour with path length.");
                });
                card(&mut c[2], "Fluid Coupling (#182 T4)", |ui| {
                    srow(ui, w2, "fluid → GI (bounce)", &params.fgi_gi, setter);
                    srow(ui, w2, "fluid shadows scene", &params.fgi_shadow, setter);
                    crow(ui, "fluid receives shadows", &params.fgi_receive, setter);
                    srow(ui, w2, "fluid sways generator", &params.fgi_sway, setter);
                    ui.label(egui::RichText::new("— caustics (liquid) —").weak().small());
                    srow(ui, w2, "caustics", &params.ca_amount, setter);
                    srow(ui, w2, "sharpness", &params.ca_sharpness, setter);
                    help(ui, "One world, one light: the medium joins the light \
                             transport. GI: glowing ink / the liquid tint the \
                             VXGI bounce (needs Voxel GI on). Shadows both ways: \
                             the ink darkens the key light on geometry, and \
                             (with cast shadows on) geometry shades the smoke. \
                             Caustics: key light refracted through the liquid \
                             surface, projected onto whatever sits beneath. \
                             Sway: the fluid pushes back — every node rides a \
                             spring driven by the local flow, so the structure \
                             moves because of the water it stirs. 'Hidden \
                             generator keeps lighting': with hide-generator on, \
                             the invisible structure still feeds probe GI, Voxel \
                             GI and the emissive-cube point lights — a pure \
                             GI/light emitter for the fluid. All inert at 0.");
                });
                card(&mut c[2], "Bloom", |ui| {
                    srow(ui, w2, "bloom", &params.bloom_intensity, setter);
                    srow(ui, w2, "threshold", &params.bloom_threshold, setter);
                });
            }); // end Look-tab columns
            } // end Look tab

            // ── Settings tab ───────────────────────────────────────
            // Infrequently-changed, per-display plumbing (none of it
            // preset-captured): column 0 = the renderer + output
            // resolution (moved from Look); column 1 = the capture /
            // production-frame stack (was the floating 🎬 panel).
            if state.tab == preset::UiTab::Settings {
            let cap_readout = state
                .render_feedback
                .as_ref()
                .and_then(|r| r.read())
                .map(|f| (f.out_w, f.out_h));
            fixed_columns(ui, |c| {
                let ws = (c[0].available_width() - COL_PAD).max(150.0);
                card(&mut c[0], "Renderer", |ui| {
                    crow(ui, "HDR output (macOS EDR)", &params.hdr_output, setter);
                    crow(ui, "wide gamut (Rec.2020)", &params.hdr_wide, setter);
                    srow(ui, ws, "vividness", &params.hdr_vivid, setter);
                    srow(ui, ws, "roll-off", &params.hdr_knee, setter);
                    param_combo(ui, ws, "tone map", &params.tonemap, setter);
                    param_combo(ui, ws, "MSAA", &params.msaa, setter);
                    help(ui, "True HDR on a HDR display/projector. Visual: 'H' also toggles. \
                             Wide gamut: tag the surface Rec.2020; Vividness then stretches \
                             colours toward those wide primaries (0 = accurate, 1 = max pop). \
                             Roll-off: lower = softer highlights, higher = punchier. \
                             Tone map applies in SDR + the HDR diffuse range.");
                });
                card(&mut c[0], "Output Resolution", |ui| {
                    crow(ui, "auto (target 60 FPS)", &params.render_auto, setter);
                    srow(ui, ws, "render scale", &params.render_scale, setter);
                    ui.label(egui::RichText::new(&res_text).color(ACCENT()).strong());
                    ui.label(egui::RichText::new("— learned upscale (Tier 5c) —").weak().small());
                    crow(ui, "learned upscale", &params.up_enable, setter);
                    srow(ui, ws, "sharpen", &params.up_sharpen, setter);
                    srow(ui, ws, "seed", &params.up_seed, setter);
                    help(ui, "Render the scene at a fraction of the output and upscale — \
                             e.g. 50% = 1080p on a 4K projector. Auto steers the scale to \
                             hold ~60 FPS; the readout above (and the visual window title) \
                             show the live resolution. Output stays native. LEARNED UPSCALE \
                             (Tier 5c) replaces the plain bilinear upscale with an HDR-safe \
                             content-adaptive sharpen (a seeded-MLP-modulated per-pixel gain) \
                             that recovers apparent detail — so auto can drop the render \
                             scale further at the same crispness. Only acts below 100% scale; \
                             SHARPEN 0 or full scale = plain bilinear. MetalFX temporal \
                             upscaling (Metal island) is the higher-quality follow-up.");
                });
                capture_ui(&mut c[1], ws, &params, setter, cap_readout);
                // Overlay strings (handle / title override) → sidecar +
                // gen bump (rides the capture stack).
                if !state.overlay_loaded {
                    state.overlay_loaded = true;
                    if let Ok(txt) = std::fs::read_to_string(ipc::overlay_sidecar_path()) {
                        state.overlay_handle = json_get(&txt, "handle");
                        state.overlay_title = json_get(&txt, "title");
                    }
                }
                card(&mut c[1], "Overlay text", |ui| {
                    let mut changed = false;
                    ui.label(egui::RichText::new("handle / watermark").weak().small());
                    changed |= ui.text_edit_singleline(&mut state.overlay_handle).changed();
                    ui.label(egui::RichText::new("title override (blank = generator name)").weak().small());
                    changed |= ui.text_edit_singleline(&mut state.overlay_title).changed();
                    if changed {
                        write_overlay_sidecar(&state.overlay_handle, &state.overlay_title, &overlay_gen);
                    }
                });
                // One home for every beat/tempo/audio-sync control (was
                // scattered across the Pulse card, the Camera Sequence card
                // and the 🎵 Audio window).
                let ws2 = (c[2].available_width() - COL_PAD).max(150.0);
                card(&mut c[2], "Sync / Tempo", |ui| {
                    crow(ui, "audio detection (analyze input)", &params.audio_react, setter);
                    ui.add_space(2.0);
                    param_combo(ui, ws2, "clock source", &params.tempo_source, setter);
                    srow(ui, ws2, "manual tempo (BPM)", &params.tempo, setter);
                    crow(ui, "PLL lock to host", &params.tempo_sync, setter);
                    srow(ui, ws2, "beats / bar", &params.beats_per_bar, setter);
                    ui.label(egui::RichText::new("— pulse (beat envelope) —").weak().small());
                    crow(ui, "pulse", &params.pulse, setter);
                    param_combo(ui, ws2, "pulse from", &params.pulse_source, setter);
                    ui.label(egui::RichText::new("— preset recall timing (#354) —").weak().small());
                    param_combo(ui, ws2, "scene timing", &params.scene_preset_timing, setter);
                    param_combo(ui, ws2, "component timing", &params.component_preset_timing, setter);
                    help(ui, "Beat-quantize preset recalls: a Scene recall snaps to SCENE TIMING; \
                             an individual Generator/Motion/Environment/Look recall snaps to \
                             COMPONENT TIMING. Instant = recall immediately. Audio / Synth / \
                             Settings recalls are always instant. (Needs the host playing; a \
                             stopped transport recalls at once.)");
                    help(ui, "One home for beat / tempo / audio sync. CLOCK SOURCE sets where \
                             the beat grid (cavity mode-morph, camera, etc.) gets its BPM: \
                             Host = follow the DAW transport; Manual = the tempo dial below; \
                             Audio = detect the BPM from the music (needs Audio detection on \
                             — holds the last BPM through a breakdown). The MANUAL TEMPO dial \
                             only takes effect in Manual mode (or Host when the DAW gives no \
                             tempo) — that's why changing it did nothing before: the default \
                             is Host, which follows the DAW's 120. PLL lock keeps the Host \
                             beat phase-aligned. PULSE is the per-beat envelope that Maxwell / \
                             Acoustic 'beat' to — from the Beat clock or the live Audio bass. \
                             Sensitivity / attack / release + pulse routing stay in the \
                             🎵 Audio window.");
                });
            }); // end Settings-tab columns
            } // end Settings tab

            // ── Audio tab (#333) — the performance instrument ──────
            if state.tab == preset::UiTab::Audio {
                audio_instrument_ui(ui, &params, setter, state, &audio_viz, &scope);
            }
        });
    });

    // The Key Map lives in a floating, closable window opened from the
    // header button, so it overlays the editor without disturbing the
    // parameter grid.
    let mut open = state.keymap_open;
    egui::Window::new("Key Map")
        .open(&mut open)
        .default_width(560.0)
        .resizable(true)
        .show(ctx, |ui| {
            keymap_ui(ui, state, &active_note);
        });
    state.keymap_open = open;

    // #356: the Performance Controller window (the four-quadrant mirror
    // grid + learn flow + diagnostics). Draws the surface state the
    // early mailbox drain already updated this frame.
    let mut perf_open = state.perf_window_open;
    egui::Window::new("Performance Controller")
        .open(&mut perf_open)
        .default_width(360.0)
        .resizable(true)
        .show(ctx, |ui| {
            perf_controller_card(ui, state, &params, setter);
        });
    state.perf_window_open = perf_open;

    // The audio-reactivity panel became the Audio tab (#333); it repaints
    // continuously while open so the meters + scope animate live.
    if state.tab == preset::UiTab::Audio {
        ctx.request_repaint();
    }

    // (The Environment and Capture floating windows became the
    // Environment and Settings tabs.)

    // Rebuild + persist the resolved key→preset map after any change
    // (a remapped key, or a preset that was saved/renamed/deleted —
    // both alter what a held note should show).
    if state.keymap_dirty {
        state.mapping.save();
        keymap.store(Arc::new(keymap::KeyMap::build(
            &state.mapping,
            &state.presets,
        )));
        state.keymap_dirty = false;
    }

    // Persist any default recorded via a ⏺ this frame (#131).
    flush_ui_defaults();
    // #593 T1 END editor body
}

/// Open a native .hdr picker WITHOUT blocking the host GUI thread. On pick,
/// write the path to the IPC sidecar and bump `hdr_gen` (GUI→atomic→process
/// handoff, mirroring `release: Arc<AtomicBool>`).
fn pick_hdr_async(hdr_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Radiance HDR", &["hdr"])
            .pick_file()
        {
            if std::fs::write(ipc::hdr_sidecar_path(), path.to_string_lossy().as_bytes()).is_ok() {
                hdr_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a material FOLDER (containing albedo/normal/roughness/metallic/ao/height
/// PNGs), write its path to the material sidecar, and bump `material_gen` (#472
/// Tier 1; mirrors `pick_hdr_async`). The visual edge-detects the counter, reads the
/// path, and (re)loads the channel maps into the GPU material texture set.
fn pick_material_async(material_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            if std::fs::write(ipc::material_sidecar_path(), dir.to_string_lossy().as_bytes()).is_ok()
            {
                material_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a connectome JSON, write its path to the connectome sidecar, and bump
/// `nn_gen` (#226 Tier 3; mirrors `pick_hdr_async`). The visual edge-detects the
/// counter, reads the path, and ingests the file via `math::neural_graph_from_json`.
fn pick_connectome_async(nn_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Network JSON", &["json"])
            // Open at the installed gallery (#226) — deploy.sh copies the demo
            // connectome/MLP/attention files here — so they're one click away.
            .set_directory(preset::networks_dir())
            .pick_file()
        {
            if std::fs::write(ipc::connectome_sidecar_path(), path.to_string_lossy().as_bytes())
                .is_ok()
            {
                nn_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a creature body-plan `.json`, write its path to the creature sidecar, and
/// bump `creature_gen` (#476 Tier 2b; mirrors `pick_connectome_async`). The visual
/// edge-detects the counter, reads the path, and rebuilds the plan via
/// `math::parse_creature_spec`.
fn pick_creature_async(creature_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Creature JSON", &["json"])
            // Open at the installed gallery (deploy.sh copies the demo body plans here).
            .set_directory(preset::creatures_dir())
            .pick_file()
        {
            if std::fs::write(ipc::creature_sidecar_path(), path.to_string_lossy().as_bytes())
                .is_ok()
            {
                creature_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a Field Playback clip `.bin`, write its path to the field-clip sidecar, and
/// bump `fieldclip_gen` (#407 Tier A; mirrors `pick_connectome_async`). The visual
/// edge-detects the counter, reads the path, and (re)loads the baked `math::FieldClip`
/// via `FieldClip::from_bytes`.
fn pick_field_clip_async(fieldclip_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Field Clip", &["bin"])
            // Open at the installed field gallery (#407) — deploy.sh copies the baked
            // demo clips here — so they're one click away.
            .set_directory(preset::fields_dir())
            .pick_file()
        {
            if std::fs::write(ipc::field_clip_sidecar_path(), path.to_string_lossy().as_bytes())
                .is_ok()
            {
                fieldclip_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a Neural CA weights JSON (#407 Tier B): write its path to the NCA sidecar
/// and bump `nca_gen`. The visual edge-detects the counter and (re)loads
/// `math::NcaWeights::from_json`, falling back to `builtin_default()` when the file
/// is missing/empty/malformed. Opens at the installed NCA gallery (`nca_dir()`), where
/// `deploy.sh` copies the demo weights. Mirrors `pick_connectome_async`.
fn pick_nca_async(nca_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("NCA weights JSON", &["json"])
            .set_directory(preset::nca_dir())
            .pick_file()
        {
            if std::fs::write(ipc::nca_sidecar_path(), path.to_string_lossy().as_bytes()).is_ok() {
                nca_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// Pick a `.gguf` model (#367 Tier 1, the visible-mind specimen): write its path to
/// the model sidecar, parse the GGUF *header* (metadata + tensor directory only — no
/// weights), fill the Mind card's readout, log the load to the mind-log, and bump
/// `model_gen`. The visual edge-detects the counter, re-parses the header, and builds
/// the architecture topology (mirrors `pick_connectome_async`). Opens by default at
/// LM Studio's model cache so existing GGUFs are one click away.
fn pick_model_async(model_gen: Arc<AtomicU32>, model_readout: Arc<std::sync::Mutex<String>>) {
    std::thread::spawn(move || {
        // Default browse dir: LM Studio's model cache — the newer `~/.lmstudio/models`
        // (LM Studio ≥ 0.3), else the older `~/.cache/lm-studio/models`, else $HOME.
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let default_dir = home
            .as_ref()
            .map(|h| h.join(".lmstudio/models"))
            .filter(|p| p.is_dir())
            .or_else(|| {
                home.as_ref()
                    .map(|h| h.join(".cache/lm-studio/models"))
                    .filter(|p| p.is_dir())
            })
            .or_else(|| home.clone())
            .unwrap_or_else(std::env::temp_dir);
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("GGUF model", &["gguf"])
            .set_directory(default_dir)
            .pick_file()
        {
            // Parse the header on this (background) thread for the card readout + log.
            let readout = match gguf::parse_file(&path) {
                Ok(h) => {
                    let text = format!(
                        "{}  ({})\nlayers {}  heads {} (kv {})\nembd {}  ff {}  vocab {}\ntensors {}  ctx {}",
                        if h.name.is_empty() { "(unnamed)" } else { &h.name },
                        if h.arch.is_empty() { "?" } else { &h.arch },
                        h.n_layers, h.n_heads, h.n_heads_kv,
                        h.n_embd, h.n_ff, h.n_vocab,
                        h.tensors.len(), h.context_length,
                    );
                    mind_log::append(
                        mind_log::MindEvent::Model,
                        "specimen",
                        &format!(
                            "loaded {} — arch {}, layers {}, heads {}, embd {}, vocab {}, tensors {}",
                            path.to_string_lossy(),
                            h.arch, h.n_layers, h.n_heads, h.n_embd, h.n_vocab, h.tensors.len()
                        ),
                    );
                    mind_log::append(
                        mind_log::MindEvent::Note,
                        "specimen",
                        &format!("parsed GGUF specimen: {} layers × {} heads", h.n_layers, h.n_heads),
                    );
                    text
                }
                Err(e) => format!("parse failed: {e}"),
            };
            if let Ok(mut g) = model_readout.lock() {
                *g = readout;
            }
            if std::fs::write(ipc::model_sidecar_path(), path.to_string_lossy().as_bytes()).is_ok() {
                model_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// #423 Tier 1 — scan a model library into the atlas. Picks a folder, header-parses
/// every `.gguf`, derives a `DesignPoint` per model against the chosen hardware
/// profile + context, writes the whole `AtlasDoc` to the atlas sidecar, then bumps
/// `atlas_gen` (write-then-bump, so the visual never edge-detects an absent file)
/// and flips the atlas on. All on a background thread (the `pick_model_async`
/// pattern) so the file dialog + parse never touch the GUI/audio thread.
fn scan_library_async(
    atlas_gen: Arc<AtomicU32>,
    atlas_on: Arc<AtomicU32>,
    atlas_readout: Arc<std::sync::Mutex<String>>,
    profile: crate::math::HardwareProfile,
    context_tokens: u32,
) {
    std::thread::spawn(move || {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let default_dir = home
            .as_ref()
            .map(|h| h.join(".lmstudio/models"))
            .filter(|p| p.is_dir())
            .or_else(|| home.clone())
            .unwrap_or_else(std::env::temp_dir);
        let Some(dir) = rfd::FileDialog::new().set_directory(default_dir).pick_folder() else {
            return;
        };
        // KV cache element size: f16 (2 bytes) — the common default. Cap the library
        // at the connectome node budget so a giant cache can't blow the graph.
        const KV_ELEM_BYTES: u64 = 2;
        const MAX_MODELS: usize = 4096;
        let (points, skipped) =
            crate::math::scan_model_library(&dir, &profile, context_tokens, KV_ELEM_BYTES, MAX_MODELS);

        let readout = if points.is_empty() {
            format!("No .gguf models found in\n{}", dir.to_string_lossy())
        } else {
            // A compact summary + the fastest/slowest attainable, honesty-tagged.
            let mut fastest = &points[0];
            let mut slowest = &points[0];
            for p in &points {
                if p.attainable_tps > fastest.attainable_tps {
                    fastest = p;
                }
                if p.attainable_tps < slowest.attainable_tps {
                    slowest = p;
                }
            }
            let unknown = points.iter().filter(|p| p.has_unknown_quant).count();
            // A model whose head geometry we had to guess still gets plotted, but its
            // roofline position is a proxy — say so rather than let it pass as derived.
            let guessed = points.iter().filter(|p| !p.kv_geometry_known).count();
            format!(
                "{} models  ({})\nprofile {} · ctx {}\n~ attainable {:.0}–{:.0} tok/s\n{}{}{}",
                points.len(),
                profile.name,
                profile.name,
                context_tokens,
                slowest.attainable_tps,
                fastest.attainable_tps,
                if skipped > 0 { format!("{skipped} skipped · ") } else { String::new() },
                if unknown > 0 { format!("{unknown} with ? quant") } else { "all quant sized".into() },
                if guessed > 0 { format!("\n? {guessed} with assumed KV geometry") } else { String::new() },
            )
        };
        if let Ok(mut g) = atlas_readout.lock() {
            *g = readout;
        }
        mind_log::append(
            mind_log::MindEvent::Note,
            "atlas",
            &format!(
                "scanned {} → {} models (profile {}, ctx {})",
                dir.to_string_lossy(),
                points.len(),
                profile.name,
                context_tokens
            ),
        );

        let doc = crate::math::AtlasDoc {
            context_tokens,
            kv_elem_bytes: KV_ELEM_BYTES as u32,
            profile,
            points,
        };
        if let Ok(json) = serde_json::to_string(&doc) {
            if std::fs::write(ipc::atlas_sidecar_path(), json.as_bytes()).is_ok() {
                atlas_on.store(1, Ordering::Relaxed);
                atlas_gen.fetch_add(1, Ordering::Relaxed);
            }
        }
    });
}

/// #423 Tier 1 — load a hardware profile from JSON (the "Load Hardware Profile…"
/// rail). Parses `{ name, bandwidth_gbps, peak_gflops }` on a background thread and
/// hands the profile to the GUI thread via the shared `loaded` slot (the card adopts
/// it into `PresetUi.atlas_custom_profile` on the next repaint); the readout carries
/// a confirmation or the parse error. It's applied to the design space by re-scanning
/// (where the derivations happen).
fn pick_hw_profile_async(
    atlas_readout: Arc<std::sync::Mutex<String>>,
    loaded: Arc<std::sync::Mutex<Option<crate::math::HardwareProfile>>>,
) {
    std::thread::spawn(move || {
        let Some(path) = rfd::FileDialog::new().add_filter("Profile JSON", &["json"]).pick_file() else {
            return;
        };
        let msg = match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| crate::math::parse_hardware_profile_json(&s))
        {
            Ok(p) => {
                let text = format!(
                    "profile loaded: {}\n{:.0} GB/s · {:.0} GFLOP/s\n(press Scan to apply)",
                    p.name, p.bandwidth_gbps, p.peak_gflops
                );
                if let Ok(mut g) = loaded.lock() {
                    *g = Some(p);
                }
                text
            }
            Err(e) => format!("profile load failed: {e}"),
        };
        if let Ok(mut g) = atlas_readout.lock() {
            *g = msg;
        }
    });
}

/// Pick a Field Engine program (#381 Tier 1): read a `.txt` expression file and
/// write its CONTENTS (the program text, not the path) to the field sidecar, then
/// bump `field_gen`. The visual edge-detects the counter and recompiles via
/// `math::FieldProgram::compile` (mirrors `pick_connectome_async`, but the sidecar
/// carries the text directly so the visual doesn't re-read a path).
fn pick_field_program_async(load_pending: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Field program", &["txt", "field"])
            .pick_file()
        {
            if let Ok(src) = std::fs::read_to_string(&path) {
                if std::fs::write(ipc::field_sidecar_path(), src.as_bytes()).is_ok() {
                    // Only signal on a successful write. The GUI thread then flips the
                    // preset to Custom and bumps `field_gen` together (see the handler),
                    // so the switch + recompile happen after the file is on disk.
                    load_pending.store(true, Ordering::Relaxed);
                }
            }
        }
    });
}

/// AI-Performer (#317 T1): APPEND the user's chat message as its own line to
/// `organic-math-chat.txt` (create+append, not truncate) and bump `chat_gen`. Appending
/// is the drop-fix (finding #3): several Sends before the visual consumes the counter all
/// survive — the visual drains every line appended since its cursor. Embedded newlines are
/// collapsed so each message stays a single line (the visual splits on lines).
fn write_chat_sidecar(msg: &str, chat_gen: &AtomicU32) {
    use std::io::Write;
    let line = format!("{}\n", msg.replace(['\r', '\n'], " "));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ipc::chat_sidecar_path())
    {
        if f.write_all(line.as_bytes()).is_ok() {
            chat_gen.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// AI-Performer (#317 T1): pick a phrase-plan JSON, write its CONTENTS to the plan sidecar
/// (`organic-math-plan.txt`), and bump `plan_gen` so the visual's debug executor
/// edge-detects + applies it (mirrors `pick_field_program_async`; the executor needs no
/// GUI-thread follow-up, so the counter is bumped straight from the picker thread).
fn pick_plan_async(plan_gen: Arc<AtomicU32>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Phrase plan", &["json"])
            .pick_file()
        {
            if let Ok(src) = std::fs::read_to_string(&path) {
                if std::fs::write(ipc::plan_sidecar_path(), src.as_bytes()).is_ok() {
                    plan_gen.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });
}

/// Write the overlay string sidecar (`{ "handle": "...", "title": "..." }`) and bump
/// `overlay_gen` so the visual re-reads it (#135 P2; mirrors the hdr sidecar).
fn write_overlay_sidecar(handle: &str, title: &str, overlay_gen: &AtomicU32) {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let json = format!("{{\"handle\":\"{}\",\"title\":\"{}\"}}", esc(handle), esc(title));
    if std::fs::write(ipc::overlay_sidecar_path(), json).is_ok() {
        overlay_gen.fetch_add(1, Ordering::Relaxed);
    }
}

/// Minimal flat-JSON string-field extractor (avoids serde for the tiny sidecar).
fn json_get(json: &str, key: &str) -> String {
    let pat = format!("\"{key}\"");
    let Some(i) = json.find(&pat) else { return String::new() };
    let after = &json[i + pat.len()..];
    let Some(c) = after.find(':') else { return String::new() };
    let rest = &after[c + 1..];
    let Some(q0) = rest.find('"') else { return String::new() };
    let mut out = String::new();
    let mut chars = rest[q0 + 1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Best-effort launch of the separate visual binary. Tries, in order: an
/// explicit env override; the copy bundled next to the plugin dylib inside the
/// `.vst3` (so the button works inside a host); a sibling of the current
/// executable (the standalone case); then `PATH`.
fn spawn_visual() {
    use std::process::Command;
    // #658 Tier 1 — spell the executable extension out rather than relying on
    // `Command`. This call site has always worked on Windows, but **by luck**: the
    // bare name is missing the `.exe` the file on disk actually carries, and it only
    // resolves because `std::process::Command` appends `EXE_SUFFIX` itself while
    // looking a program up. That is an implementation detail one layer down, and its
    // sibling `mind_runtime_path()` two functions below reaches the same directories
    // through `Path::exists()`, which does **not** do it — so the identical bare name
    // was silently always-false there. Naming the suffix in both is what stops the two
    // from drifting apart again; don't "simplify" it back out.
    // `EXE_SUFFIX` is `""` on macOS and Linux, so every path below is byte-identical there.
    let visual_bin = format!("organic-math-visual{}", std::env::consts::EXE_SUFFIX);
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("ORGANIC_MATH_VISUAL") {
        candidates.push(p.into());
    }
    if let Some(dir) = current_dylib_dir() {
        candidates.push(dir.join(&visual_bin));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&visual_bin));
        }
    }
    candidates.push(visual_bin.into());
    // #483 Tier 1 — hand the child OUR IPC namespace. The visual is compiled once
    // (feature-off, so its own `EDITION` is `Full`) and shipped byte-identical to both
    // products; passing `$ORGANON_IPC_NS` is what points it at the Organon Mind
    // snapshot instead of Organon's when a Mind editor spawned it. Under full Organon
    // this sets the variable to `organic-math` — exactly what the child would have
    // resolved on its own, so behaviour is unchanged.
    let ns = crate::ipc::namespace();
    for path in candidates {
        if Command::new(&path).env("ORGANON_IPC_NS", ns).spawn().is_ok() {
            return;
        }
    }
    nih_warn!("Could not launch organic-math-visual — set ORGANIC_MATH_VISUAL or run it manually.");
}

/// #367 Tier 2 — locate the embedded `organic-math-mind-runtime` helper the same
/// way `spawn_visual` finds the visual: an env override, then next to the plugin
/// dylib (where `bundle.sh --with-llm` embeds it in `Contents/MacOS/`), then next
/// to the current exe, then `$PATH`. Returns the first path that exists so the
/// Mind card can report "not bundled" cleanly when the plugin was built without
/// `--with-llm`.
fn mind_runtime_path() -> Option<std::path::PathBuf> {
    // #658 Tier 1 — this one was a real bug, not a tidiness point. Unlike `spawn_visual`
    // above, the candidates here are filtered by `Path::exists()`, which asks the
    // filesystem the literal question and gets a literal answer: the bundled file is
    // `organic-math-mind-runtime.exe` on Windows, so a probe for the extension-less name
    // **never** matched and the Mind card reported "not bundled" even on a
    // `--with-llm` build that had embedded it right there. `EXE_SUFFIX` is `""` on
    // macOS and Linux, so this is a no-op on both.
    let runtime_bin = format!("organic-math-mind-runtime{}", std::env::consts::EXE_SUFFIX);
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("ORGANIC_MATH_MIND_RUNTIME") {
        candidates.push(p.into());
    }
    if let Some(dir) = current_dylib_dir() {
        candidates.push(dir.join(&runtime_bin));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(&runtime_bin));
        }
    }
    candidates.into_iter().find(|p| p.exists())
}

/// The directory containing this loaded plugin binary. Inside a `.vst3` that's
/// `Contents/MacOS/` on the Mac and `Contents/x86_64-win/` on Windows — either way,
/// where the bundler also drops `organic-math-visual`.
///
/// **Why not `current_exe()`.** In a host the plugin is a *library* loaded into
/// someone else's process, so `current_exe()` is Ableton, not us. The question
/// "which file on disk is this code in?" has to be asked of the dynamic loader, and
/// every platform answers it the same way: hand it an address known to lie inside
/// this module and let it name the module that owns it. `current_dylib_dir` itself is
/// that address — the function takes its own address purely as a landmark.
#[cfg(unix)]
fn current_dylib_dir() -> Option<std::path::PathBuf> {
    use std::ffi::{CStr, OsStr};
    use std::os::unix::ffi::OsStrExt;
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    let addr = current_dylib_dir as *const libc::c_void;
    if unsafe { libc::dladdr(addr, &mut info) } != 0 && !info.dli_fname.is_null() {
        let cstr = unsafe { CStr::from_ptr(info.dli_fname) };
        let path = std::path::PathBuf::from(OsStr::from_bytes(cstr.to_bytes()));
        return path.parent().map(|p| p.to_path_buf());
    }
    None
}

/// #658 Tier 1 — the Windows arm, structurally the same trick as the `dladdr` one above.
///
/// `GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS` is Win32's `dladdr`: it reinterprets the
/// `lpModuleName` argument as an *address* rather than a name and returns the `HMODULE`
/// whose image contains it. Pairing it with `…_UNCHANGED_REFCOUNT` is what makes the
/// call a pure query — without that flag `GetModuleHandleExW` takes a reference on the
/// module, and since nothing here ever calls `FreeLibrary` the plugin would pin itself
/// in the host's address space forever and never unload. `GetModuleFileNameW` then turns
/// the handle into the DLL's full path, which is the string `dli_fname` hands back for free.
///
/// Until this existed the `cfg(not(unix))` stub returned `None`, so on Windows
/// "Open Visual Window" fell through to `current_exe()` (the host) and `PATH` (nothing
/// installed there) and could not find the visual embedded beside the DLL in the `.vst3`.
#[cfg(windows)]
fn current_dylib_dir() -> Option<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };

    let mut module: HMODULE = std::ptr::null_mut();
    // The cast is the whole point: this is an address inside our image, not a name.
    let addr = current_dylib_dir as *const u16;
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            addr,
            &mut module,
        )
    };
    if ok == 0 {
        return None;
    }

    // `GetModuleFileNameW` does not report the length it needs — on overflow it fills the
    // buffer, returns `nSize` (the return value excludes the NUL, so a perfect fit is
    // `nSize - 1`) and sets `ERROR_INSUFFICIENT_BUFFER`. So the only correct shape is
    // grow-and-retry. `MAX_PATH` covers a normal install in one pass; a `\\?\`-prefixed
    // or deeply nested VST3 folder takes a couple more. 32 767 wchars is the extended-path
    // ceiling — a path that still doesn't fit there is not going to, so stop rather than spin.
    let mut buf: Vec<u16> = vec![0u16; 260];
    loop {
        let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) };
        if len == 0 {
            return None;
        }
        if (len as usize) < buf.len() {
            buf.truncate(len as usize);
            break;
        }
        let truncated = unsafe { GetLastError() } == ERROR_INSUFFICIENT_BUFFER;
        if !truncated || buf.len() >= 32_768 {
            return None;
        }
        buf.resize(buf.len() * 2, 0);
    }

    // UTF-16 → `OsString` losslessly; Windows paths are not guaranteed well-formed UTF-8,
    // so this is the counterpart of the Unix arm's `OsStr::from_bytes`, not `String::from_utf16`.
    let path = std::path::PathBuf::from(OsString::from_wide(&buf));
    path.parent().map(|p| p.to_path_buf())
}

/// Any third platform (wasm, and whatever comes next): no loader to ask, so the
/// callers fall back to `current_exe()` / `PATH`. Kept deliberately — narrowing it to
/// `not(any(unix, windows))` is what lets the two real arms above exist without a
/// duplicate-definition error.
#[cfg(not(any(unix, windows)))]
fn current_dylib_dir() -> Option<std::path::PathBuf> {
    None
}

/// Amber accent, matching the web app (#ffb547).
///
/// #542 Tier 1 moved the value into `theme::AMBER` and narrowed what it *means*: it marks
/// live state (streaming indicators, meter mid-scale, the active tab), not every card
/// title. #551 Tier 1 made it live state, so this is a function rather than a const —
/// kept under the same name so the remaining call sites read unchanged.
#[allow(non_snake_case)]
fn ACCENT() -> egui::Color32 { theme::AMBER() }

/// #520 Tier 2 — the floor the editor window may be resized to, in points.
///
/// The *default* size is unchanged (`EguiState::from_size(1280, 860)` in
/// `params.rs`); this is only how small a drag is allowed to make it. Chosen so a
/// card is still readable rather than so the window is still legal: `fixed_columns`
/// splits the width three ways and subtracts `COL_PAD` per column, so much below
/// ~640 the three columns stop being able to hold a slider row and its label. The
/// height floor keeps the tab bar plus a card's first few rows on screen.
///
/// Both the standalone (`NSWindow` `contentMinSize`) and the plugin
/// (`ResizableWindow::min_size`) use these, so the two products cannot be squeezed
/// to different limits.
const MIN_EDITOR_W: f64 = 640.0;
const MIN_EDITOR_H: f64 = 480.0;

/// House style. Cheap + idempotent, so it's fine to (re)apply each frame.
///
/// #542 Tier 1 moved the body into [`theme::install`], which additionally installs the
/// Inter type ramp (once per context — it rebuilds the glyph atlas).
fn apply_theme(ctx: &egui::Context) {
    theme::install(ctx);
}

/// Lay out exactly three **equal fixed-width** columns that fill the available
/// tab width edge-to-edge. Like `ui.columns(3, …)`, but content can never change
/// the column width: the width is derived from the window size alone (floored,
/// min `CARD_COL_MIN_W`), each column's child UI is pinned to its own rect, and
/// clipped to its horizontal strip — so cards keep a stable width and never
/// reflow or overlap when a slider readout changes length. Cards dock into
/// `cols[0..3]`, exactly as with `ui.columns`.
///
/// Mirrors egui's own `columns_dyn`: three child UIs at fixed x-offsets, then one
/// `allocate_rect` to reserve the tallest column's height so the surrounding
/// `ScrollArea` sizes correctly.
fn fixed_columns(ui: &mut egui::Ui, add: impl FnOnce(&mut [egui::Ui])) {
    let spacing = ui.spacing().item_spacing.x;
    let col_w = (((ui.available_width() - 2.0 * spacing) / 3.0).floor()).max(CARD_COL_MIN_W);
    let top_left = ui.cursor().min;
    let bottom = ui.max_rect().bottom();

    let mut columns: Vec<egui::Ui> = (0..3)
        .map(|i| {
            let x = top_left.x + (i as f32) * (col_w + spacing);
            let rect = egui::Rect::from_min_max(
                egui::pos2(x, top_left.y),
                egui::pos2(x + col_w, bottom),
            );
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::top_down_justified(egui::Align::LEFT)),
            );
            col.set_width(col_w);
            // Clip the column to its own horizontal strip (x only — the scroll
            // area owns the vertical clip): even if some widget still overflows,
            // it can never paint across the seam into the next column.
            let mut clip = col.clip_rect();
            clip.min.x = clip.min.x.max(rect.left() - 1.0);
            clip.max.x = clip.max.x.min(rect.right() + 1.0);
            col.set_clip_rect(clip);
            col
        })
        .collect();

    add(&mut columns[..]);

    let max_h = columns.iter().fold(0.0_f32, |h, c| h.max(c.min_size().y));
    let total_w = 3.0 * col_w + 2.0 * spacing;
    ui.allocate_rect(
        egui::Rect::from_min_size(top_left, egui::vec2(total_w, max_h)),
        egui::Sense::hover(),
    );
}

/// A titled "card": a filled, grouped frame with an amber collapsing header.
/// Replaces the bare collapsing sections so the panel reads as grouped controls
/// rather than one long wall of sliders.

/// The **Neural Network generator** card (#226), shared by two call sites (#520 Tier 1).
///
/// It lives on the Generator tab, where it appears when that generator is selected — and
/// on the **Mind tab**, where it leads column 0 unconditionally, because Organon Mind's
/// generator is *always* a neural network and its controls should not be a tab away.
/// One body, two call sites: the two can never drift apart.
fn neural_network_card(
    col: &mut egui::Ui,
    w0: f32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    nn_gen: &Arc<AtomicU32>,
) {
        card(col, "Neural Network", |ui| {
            param_combo_sized(ui, w0, "topology", &params.nw_topology, setter, 2.0 * COMBO_W);
            if ui
                .button("Load Network (JSON)…")
                .on_hover_text(
                    "Ingest a real network. A CONNECTOME — {nodes:[{id,pos?,\
                     scalar?}], edges:[{src,dst,weight?}]} (e.g. C. elegans) \
                     → set topology = Connectome. A trained MLP — {layers:\
                     [..], weights:[[..],..], biases?, activation?, input?} → \
                     set topology = MLP (loaded weights): the layers lay out \
                     left-to-right, edges are the signed weights, and a live \
                     forward pass lights the nodes. Or an ATTENTION tensor — \
                     {type:\"attention\", tokens, layers, heads, attention:\
                     [L][H][T][T]} → set topology = Attention (transformer): \
                     tokens become a triangular causal attention graph.",
                )
                .clicked()
            {
                pick_connectome_async(nn_gen.clone());
            }
            srow(ui, w0, "nodes", &params.nw_nodes, setter);
            srow(ui, w0, "connectivity (k)", &params.nw_connectivity, setter);
            srow(ui, w0, "rewire / radius", &params.nw_rewire, setter);
            srow(ui, w0, "layers (layered)", &params.nw_layers, setter);
            srow(ui, w0, "extent", &params.nw_extent, setter);
            srow(ui, w0, "seed", &params.nw_seed, setter);
            ui.label(egui::RichText::new("— nodes (soma) —").weak().small());
            srow(ui, w0, "node size", &params.nw_node_size, setter);
            srow(ui, w0, "node glow", &params.nw_node_glow, setter);
            ui.label(egui::RichText::new("— edges (tracts) —").weak().small());
            srow(ui, w0, "thickness", &params.nw_edge_thickness, setter);
            srow(ui, w0, "bow", &params.nw_edge_bow, setter);
            srow(ui, w0, "samples/edge", &params.nw_edge_samples, setter);
            ui.label(egui::RichText::new("— travelling pulse —").weak().small());
            srow(ui, w0, "pulse speed", &params.nw_pulse_speed, setter);
            srow(ui, w0, "pulse width", &params.nw_pulse_width, setter);
            ui.label(egui::RichText::new("— axon bundle + dendrites (T1.5) —").weak().small());
            srow(ui, w0, "edge fibres", &params.nw_edge_fibres, setter);
            srow(ui, w0, "bundle radius", &params.nw_bundle_radius, setter);
            srow(ui, w0, "ranvier dip", &params.nw_edge_node_dip, setter);
            srow(ui, w0, "ranvier nodes", &params.nw_ranvier, setter);
            srow(ui, w0, "dendrite", &params.nw_dendrite, setter);
            srow(ui, w0, "dendrite count", &params.nw_dendrite_count, setter);
            ui.label(egui::RichText::new("— signal propagation (Tier 2) —").weak().small());
            param_combo_sized(ui, w0, "firing", &params.nw_fire_mode, setter, 2.0 * COMBO_W);
            srow(ui, w0, "threshold", &params.nw_threshold, setter);
            srow(ui, w0, "conduction", &params.nw_conduction, setter);
            srow(ui, w0, "refractory", &params.nw_refractory, setter);
            srow(ui, w0, "decay", &params.nw_decay, setter);
            srow(ui, w0, "deposit", &params.nw_deposit, setter);
            srow(ui, w0, "stimulus rate", &params.nw_stim_rate, setter);
            srow(ui, w0, "signal motes", &params.nw_motes, setter);
            ui.label(egui::RichText::new("— MLP: real weights (Tier 4) —").weak().small());
            srow(ui, w0, "sign colour", &params.nw_sign_colour, setter);
            srow(ui, w0, "sparsify", &params.nw_sparsify, setter);
            srow(ui, w0, "layer gap", &params.nw_layer_gap, setter);
            srow(ui, w0, "input drive", &params.nw_mlp_drive, setter);
            ui.label(egui::RichText::new("— attention: transformer (Tier 5) —").weak().small());
            srow(ui, w0, "layer", &params.nw_attn_layer, setter);
            srow(ui, w0, "head", &params.nw_attn_head, setter);
            srow(ui, w0, "edge threshold", &params.nw_attn_threshold, setter);
            srow(ui, w0, "tokens (synth)", &params.nw_attn_tokens, setter);
            srow(ui, w0, "reveal /beat", &params.nw_attn_reveal, setter);
            srow(ui, w0, "head sweep /beat", &params.nw_attn_sweep, setter);
            srow(ui, w0, "ring layout", &params.nw_attn_ring, setter);
            ui.label(egui::RichText::new("— brain model (#275) —").weak().small());
            srow(ui, w0, "fold depth", &params.br_fold_depth, setter);
            srow(ui, w0, "fold freq (gyri)", &params.br_fold_freq, setter);
            srow(ui, w0, "fissure", &params.br_hemi_gap, setter);
            srow(ui, w0, "local k", &params.br_local_k, setter);
            srow(ui, w0, "cerebellum", &params.br_cerebellum, setter);
            srow(ui, w0, "assoc tracts", &params.br_assoc, setter);
            srow(ui, w0, "corpus callosum", &params.br_callosum, setter);
            srow(ui, w0, "subcortical", &params.br_subcortical, setter);
            srow(ui, w0, "target highlight", &params.br_region_hi, setter);
            srow(ui, w0, "target region", &params.br_target, setter);
            srow(ui, w0, "stim strength", &params.br_stim_amount, setter);
            srow(ui, w0, "stim rate /beat", &params.br_stim_rate, setter);
            srow(ui, w0, "signal swell", &params.br_signal_swell, setter);
            help(ui, "Signal swell = how much a firing soma physically swells (0 = \
                     glow only, the brain holds still while signals propagate; raise \
                     for the pulsing 'living tissue' look). Brain topology only.");
            help(ui, "A graph of neuron nodes (soma blobs) wired by edges — routed \
                     fibre tracts, the Axon Waveguide edge at network scale. Pick a \
                     synthetic topology: a random-geometric neuron cloud, a layered \
                     feed-forward net (the ANN layout), a ring lattice, or a \
                     Watts–Strogatz small-world (rewire from the ring). Connectivity \
                     sets neighbours (ring/small-world) or fan-out (layered); rewire \
                     is the WS probability (or the connection radius for the cloud). \
                     Hub nodes read bigger + brighter; a pulse travels each tract. \
                     T1.5: raise 'edge fibres' to render each edge as a MYELINATED \
                     BUNDLE (the real Axon Waveguide tract — Ranvier nodes, staggered \
                     pulse) instead of one tube, and 'dendrite' to sprout an arbor \
                     from each soma so nodes read as neurons, not blobs. \
                     Signal propagation (Tier 2): set a firing mode and watch \
                     activation cascade through the graph — a node fires, its edges \
                     carry the pulse, the target fires after a conduction delay \
                     (threshold + refractory); Wavefront sweeps, Oscillation idles, \
                     Stimulus ripples out; motes ride the active edges. \
                     Tier 3: Load a real CONNECTOME (C. elegans) or a Cortical \
                     sheet. Tier 4: Load a trained MLP (topology = MLP) — its \
                     layers lay out left-to-right, edges are the SIGNED weights \
                     (warm = +, cool = −; thickness = magnitude, sparsified to \
                     read), and a live forward pass lights the nodes with the \
                     real activations (input drive breathes it on the beat). \
                     Tier 5: Load an ATTENTION tensor (topology = Attention) — or \
                     leave it unloaded for a stylized causal synthesis. Tokens \
                     are nodes on a row (or ring); causal attention edges (i→j, \
                     j≤i) carry the weight A_ij, a residual backbone links \
                     consecutive tokens, and each token glows by how attended-to \
                     it is (the BOS sink lights up). 'reveal /beat' grows the \
                     attended set token-by-token; 'head sweep' auto-cycles heads. \
                     Best in Swept Tubes + Glass/HDR. Honest framing: this renders \
                     connectivity + activity, not a neural simulation. For the MLP \
                     the GRAPH is real (its actual weights) but the 3-D layout is \
                     imposed — units, not cells; likewise attention shows a real \
                     (or plausible) attention pattern over token positions — not \
                     a claim the network 'thinks'. BRAIN MODEL (#275): topology = \
                     Brain model builds two folded cerebral hemispheres split by a \
                     longitudinal fissure, plus a cerebellum + brainstem, wired \
                     short-range local cortex — the substrate the TMS + entrainment \
                     tools will stimulate. Best in the Neural Tissue surface. \
                     Stylized anatomy: plausible + beautiful, NOT an accurate brain.");
        });
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::framed(ui, |ui| {
        ui.set_width(ui.available_width());
        // The header gets its own painted band — the #542 §5 three-stop silver gradient,
        // whose middle stop is lighter than both ends so it reads as convex rolled metal.
        // Painted behind the header row once its rect is known, same deferred dance as the
        // card body (`theme::framed`).
        let band = ui.painter().add(egui::Shape::Noop);
        let head = egui::CollapsingHeader::new(theme::card_title(title))
            .default_open(true)
            .show_unindented(ui, add);
        // The band spans the header row only, bleeding to the card's inner edges so it reads
        // as a machined band rather than a floating pill.
        let m = theme::CARD_INNER_MARGIN;
        let hr = head.header_response.rect;
        let band_rect = egui::Rect::from_min_max(
            egui::pos2(ui.max_rect().left() - m.0, hr.top() - m.1),
            egui::pos2(ui.max_rect().right() + m.0, hr.bottom() + m.1 * 0.5),
        );
        ui.painter().set(band, theme::card_header_band(band_rect));
    });
    ui.add_space(6.0);
}

// ---------------------------------------------------------------------------
// User-recorded per-parameter defaults (#131)
//
// The ⏺ button on a numeric slider records that param's current value as its
// default; the ⟲ reset then targets the recorded value (else the factory one).
// Persistence is keyed by the param's nih-plug **id** so it survives a reload —
// but a widget only has `&Param`, not its id. Rather than thread an id through
// ~200 call sites, we build a `ParamPtr-hash → id` map once from
// `params.param_map()` and recover the id at the widget via `Param::as_ptr()`.
// The context lives in a GUI-thread-local; `ensure_ui_defaults` rebuilds it when
// the params instance changes (the GUI thread can be shared across plugin
// instances), so the map always matches the editor currently drawing.
// ---------------------------------------------------------------------------

thread_local! {
    static UI_DEFAULTS: RefCell<Option<UiDefaults>> = const { RefCell::new(None) };
}

struct UiDefaults {
    /// Identity of the params instance this context was built for.
    params_key: usize,
    /// hash(ParamPtr) → param id, so a `&Param` can find its id.
    id_of: HashMap<u64, String>,
    /// The recorded-defaults overlay (id → normalized).
    store: preset::Defaults,
    /// A ⏺ recorded something this frame → persist on flush.
    dirty: bool,
}

/// Stable hash of any `Hash` value (here a `ParamPtr`) → u64, so we can key by
/// `ParamPtr` without naming the type.
fn hash_of<T: std::hash::Hash>(t: &T) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// (Re)build the editor's defaults context if absent or built for a different
/// params instance. Cheap (~390 entries) and only rebuilds on an instance switch.
fn ensure_ui_defaults(params: &OrganicMathParams) {
    let key = params as *const OrganicMathParams as usize;
    UI_DEFAULTS.with(|c| {
        let mut c = c.borrow_mut();
        let stale = c.as_ref().map(|u| u.params_key != key).unwrap_or(true);
        if stale {
            let mut id_of = HashMap::new();
            for (id, ptr, _group) in params.param_map() {
                id_of.insert(hash_of(&ptr), id.to_string());
            }
            *c = Some(UiDefaults {
                params_key: key,
                id_of,
                store: preset::Defaults::load(),
                dirty: false,
            });
        }
    });
}

/// Persist the recorded defaults if a ⏺ changed them this frame.
fn flush_ui_defaults() {
    UI_DEFAULTS.with(|c| {
        if let Some(u) = c.borrow_mut().as_mut() {
            if u.dirty {
                u.store.save();
                u.dirty = false;
            }
        }
    });
}

/// The recorded normalized default for a param, if the user has set one.
fn recorded_default<P: Param>(param: &P) -> Option<f32> {
    UI_DEFAULTS.with(|c| {
        c.borrow().as_ref().and_then(|u| {
            u.id_of
                .get(&hash_of(&param.as_ptr()))
                .and_then(|id| u.store.map.get(id).copied())
        })
    })
}

/// Record the param's current value as its user default (persisted on flush).
fn record_default<P: Param>(param: &P) {
    let v = param.unmodulated_normalized_value();
    UI_DEFAULTS.with(|c| {
        if let Some(u) = c.borrow_mut().as_mut() {
            if let Some(id) = u.id_of.get(&hash_of(&param.as_ptr())).cloned() {
                u.store.map.insert(id, v);
                u.dirty = true;
            }
        }
    });
}

/// Reset one parameter to its default (gesture-safe, so the host records it).
/// Targets the user-recorded default when present (#131), else the factory one.
fn reset_one<P: Param>(param: &P, setter: &ParamSetter) {
    let norm = recorded_default(param).unwrap_or_else(|| param.default_normalized_value());
    setter.begin_set_parameter(param);
    setter.set_parameter_normalized(param, norm);
    setter.end_set_parameter(param);
}

/// The little per-control reset affordance. Tooltip notes when a recorded
/// default is in play so ⟲'s target isn't surprising.
fn reset_btn(ui: &mut egui::Ui) -> bool {
    ui.small_button("⟲")
        .on_hover_text("Reset to default (your recorded default if set, else factory)")
        .clicked()
}

/// What the merged default button (`default_btn`) did this frame.
enum DefaultAction {
    Reset,
    Record,
}

/// The merged per-control default button (#131, compressed from two buttons):
/// click = reset to default (recorded if set, else factory); **hold ⌘ (Cmd /
/// Ctrl) and click = record** the current value as the control's default. The
/// glyph flips ⟲ → ● while the modifier is held so the mode is visible before
/// committing. (`●` U+25CF renders in egui's default fonts; the media-record
/// glyph U+23FA is often missing and shows as tofu.)
fn default_btn(ui: &mut egui::Ui) -> Option<DefaultAction> {
    let record = ui.input(|i| i.modifiers.command);
    let (glyph, hint) = if record {
        ("●", "Record the current value as this control's default")
    } else {
        (
            "⟲",
            "Reset to default (recorded if set, else factory). Hold ⌘ and click to record \
             the current value as the default.",
        )
    };
    let clicked = ui.small_button(glyph).on_hover_text(hint).clicked();
    clicked.then(|| if record { DefaultAction::Record } else { DefaultAction::Reset })
}

/// Inert placeholder for the two upcoming per-row actions (modulation routing
/// etc.) — laid out now so every row already has its final three-button width;
/// greyed out until wired.
// #542 Tier 1 removed `placeholder_btn` — two permanently-disabled buttons labelled
// "1"/"2" that `srow` and `param_combo_sized` drew on every one of ~1057 control rows.
// They did nothing (`add_enabled(false)`, no click path), and the ~56 pt they cost per
// row was the only slack in the grid — reclaiming it is what lets labels set whole
// instead of reading `connecti…` / `ranvier n…`.
//
// They read as placeholders for per-row modulation-slot assignment. When that ships it
// wants an indicator that lights when the row is *routed* (and is absent when it isn't),
// not two permanently dead buttons on every row — so this is a deliberate product call,
// not an oversight. See #542.

/// Card description, compressed behind a small "?" (sits at the bottom-left of
/// its card). Hover shows the text as a tooltip; click pins it open as an
/// in-card bubble; clicking the "?" (or the bubble itself) again collapses it.
fn help(ui: &mut egui::Ui, text: &str) {
    let id = ui.id().with("help_open").with(text);
    let mut open = ui.data_mut(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if ui.small_button("?").on_hover_text(text).clicked() {
        open = !open;
    }
    if open {
        let bubble = ui.add(
            egui::Label::new(egui::RichText::new(text).weak().small())
                .sense(egui::Sense::click()),
        );
        if bubble.clicked() {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, open));
}

/// Apply a parameter set **atomically** with respect to the visual snapshot:
/// bump the seqlock generation odd before mutating params and even after, so
/// `process()` never publishes a snapshot it captured mid-apply (which read as a
/// multi-frame stutter — shape updating a few frames before colour). The bracket
/// covers the whole `apply()`, so any audio block that runs during it is skipped
/// and the next stable block writes the complete new look in one step.
fn apply_atomic(
    apply_gen: &AtomicU32,
    values: &preset::PresetValues,
    params: &OrganicMathParams,
    setter: &ParamSetter,
) {
    apply_gen.fetch_add(1, Ordering::AcqRel); // → odd: applying
    values.apply(params, setter);
    apply_gen.fetch_add(1, Ordering::AcqRel); // → even: stable
}

/// Recall only one tab's parameters (#145), under the same seqlock as a full
/// recall so the visual never renders a half-applied partial state.
fn apply_tab_atomic(
    apply_gen: &AtomicU32,
    tab: preset::EditorTab,
    values: &preset::PresetValues,
    params: &OrganicMathParams,
    setter: &ParamSetter,
) {
    apply_gen.fetch_add(1, Ordering::AcqRel); // → odd: applying
    values.apply_tab(tab, params, setter);
    apply_gen.fetch_add(1, Ordering::AcqRel); // → even: stable
}

/// Perform a preset recall (#354): apply the scope's params under the seqlock,
/// then restore the loaded `.hdr` for recalls that touch the Environment bucket
/// (Scene or Environment) by re-driving the sidecar + bumping `hdr_gen`. Shared
/// by the immediate and beat-quantized recall paths.
fn apply_recall(
    apply_gen: &AtomicU32,
    scope: preset::PresetScope,
    v: &preset::PresetValues,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    hdr_gen: &Arc<AtomicU32>,
) {
    match scope {
        preset::PresetScope::Global => apply_atomic(apply_gen, v, params, setter),
        preset::PresetScope::Tab(tab) => apply_tab_atomic(apply_gen, tab, v, params, setter),
    }
    let touches_env = matches!(
        scope,
        preset::PresetScope::Global | preset::PresetScope::Tab(preset::EditorTab::Environment)
    );
    if touches_env && !v.hdr_path.is_empty() {
        let _ = std::fs::write(crate::ipc::hdr_sidecar_path(), &v.hdr_path);
        hdr_gen.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether a scheduled recall applied immediately or was queued for a boundary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RecallOutcome {
    Applied,
    Queued,
}

/// Schedule a beat-quantized recall (#354). Picks the division for `scope` (Scene
/// → Scene-timing, Scene-component → Component-timing, everything else instant),
/// then either applies immediately (instant division or stopped transport) or arms
/// `state.pending_recall` for the next boundary. A new recall supersedes any queued
/// one. Extracted from `presets_ui` so BOTH the mouse rail and the #356 controller
/// mailbox drain schedule recalls through one path.
fn enqueue_recall(
    state: &mut preset::PresetUi,
    scope: preset::PresetScope,
    values: preset::PresetValues,
    apply_gen: &AtomicU32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    hdr_gen: &Arc<AtomicU32>,
    beat_pos: &Arc<AtomicU32>,
) -> RecallOutcome {
    use preset::PresetScope;
    // A new recall supersedes any queued one (else a stale scheduled recall could
    // still fire later and overwrite what the user just applied). Clearing the
    // pad-feedback marker here keeps it in lock-step with `pending_recall`: it is
    // only ever re-set by the controller drain when THIS recall it just scheduled
    // was deferred, so the mirror grid can never promote a superseded pad.
    state.pending_recall = None;
    state.perf_queued = None;
    let div = match scope {
        PresetScope::Global => params.scene_preset_timing.value(),
        PresetScope::Tab(tab) if tab.in_scene() => params.component_preset_timing.value(),
        PresetScope::Tab(_) => crate::params::PresetDivision::Instant,
    };
    let step = div.beats(params.beats_per_bar.value() as f32);
    let now = f32::from_bits(beat_pos.load(Ordering::Relaxed));
    if step <= 0.0 || now < 0.0 {
        apply_recall(apply_gen, scope, &values, params, setter, hdr_gen);
        RecallOutcome::Applied
    } else {
        let target = ((now / step).floor() + 1.0) * step;
        state.pending_recall = Some((scope, values, target, now)); // high-water = now
        RecallOutcome::Queued
    }
}

/// Fire an armed recall once the beat crosses its boundary, the transport stops,
/// OR it jumps backward (loop/seek — the high-water mark catches any backward
/// move so a looped region can't strand it). Returns the scope that fired, if any.
/// `repaint` is `Some(ctx)` from an egui frame (keep polling until the boundary);
/// a headless caller passes `None`. Extracted from `presets_ui` alongside
/// `enqueue_recall` so a controller-scheduled recall still fires.
fn poll_pending_recall(
    state: &mut preset::PresetUi,
    apply_gen: &AtomicU32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    hdr_gen: &Arc<AtomicU32>,
    beat_pos: &Arc<AtomicU32>,
    repaint: Option<&egui::Context>,
) -> Option<preset::PresetScope> {
    if let Some((scope, v, target, high)) = state.pending_recall.clone() {
        let now = f32::from_bits(beat_pos.load(Ordering::Relaxed));
        let jumped_back = now + 1.0e-3 < high;
        if now < 0.0 || now >= target || jumped_back {
            apply_recall(apply_gen, scope, &v, params, setter, hdr_gen);
            state.pending_recall = None;
            return Some(scope);
        } else {
            state.pending_recall = Some((scope, v, target, high.max(now))); // raise high-water
            if let Some(ctx) = repaint {
                ctx.request_repaint(); // keep polling until the boundary
            }
        }
    }
    None
}

/// Highest bank index the ◀▶ paging reaches (banks of 16 → 256 slots).
const MAX_PERF_BANK: usize = 15;

/// #356: step the Component-timing division param by ±1 (the ▲▼ arrows in
/// Component mode). Writes through the host setter so it's automation-recordable
/// and the on-screen dropdown mirrors it.
fn step_component_division(params: &OrganicMathParams, setter: &ParamSetter, delta: i32) {
    let p = &params.component_preset_timing;
    let steps = p.step_count().unwrap_or(0) as i32; // variant count - 1
    if steps <= 0 {
        return;
    }
    let cur = (p.unmodulated_normalized_value() * steps as f32).round() as i32;
    let next = (cur + delta).clamp(0, steps);
    setter.begin_set_parameter(p);
    setter.set_parameter_normalized(p, next as f32 / steps as f32);
    setter.end_set_parameter(p);
}

/// #356: drain the performance-controller mailbox and act on each surface gesture.
/// Runs every editor frame (before any panel draws). Routes raw MIDI through the
/// device profile, feeds the learn flow, schedules quantized component/scene
/// recalls through the shared `enqueue_recall`, and reconciles the mirror-grid
/// feedback (queued → active once the pending recall fires).
/// Apply ONE agent `ApplyOp` to the real params via the host `ParamSetter`, on the GUI
/// thread — so the sliders / dropdowns mirror exactly what the AI Performer did (#317
/// UI-sync). Params are the single source of truth; `to_shared` then carries the value to
/// the visual, so the display can never disagree with the sliders. `set_parameter` clamps
/// to each param's own range; ids are the flat `param_block!` field names (see
/// `agent::actuate`). Unknown ids are ignored.
fn apply_agent_change(params: &OrganicMathParams, setter: &ParamSetter, op: &agent::ApplyOp) {
    use agent::ApplyOp;
    use crate::params::{HostCamPath, HostGeneratorMode, HostMaterialType, HostSurfaceMode};
    use nih_plug::prelude::Enum;
    macro_rules! set {
        ($p:expr, $v:expr) => {{
            let p = $p;
            setter.begin_set_parameter(p);
            setter.set_parameter(p, $v);
            setter.end_set_parameter(p);
        }};
    }
    match op {
        ApplyOp::Generator(i) => {
            set!(&params.generator, HostGeneratorMode::from_index(*i as usize))
        }
        ApplyOp::Surface(i) => set!(&params.surface_mode, HostSurfaceMode::from_index(*i as usize)),
        ApplyOp::Material(i) => set!(&params.mat_type, HostMaterialType::from_index(*i as usize)),
        ApplyOp::Release => {} // values stay put; the editor's own button clears holds
        ApplyOp::Set(id, v) => {
            let v = *v;
            match id.as_str() {
                // IntParams (loop counts) — set with the truncated integer.
                "loop_count_x" => set!(&params.loop_count_x, v as i32),
                "loop_count_y" => set!(&params.loop_count_y, v as i32),
                "loop_count_z" => set!(&params.loop_count_z, v as i32),
                "loop_count_q" => set!(&params.loop_count_q, v as i32),
                // FloatParams — field name == agent id (verified against `agent::actuate`).
                "rot_amp_x" => set!(&params.rot_amp_x, v),
                "rot_amp_y" => set!(&params.rot_amp_y, v),
                "rot_amp_z" => set!(&params.rot_amp_z, v),
                "rot_mod_x" => set!(&params.rot_mod_x, v),
                "rot_mod_y" => set!(&params.rot_mod_y, v),
                "rot_mod_z" => set!(&params.rot_mod_z, v),
                "trans_amp_x" => set!(&params.trans_amp_x, v),
                "trans_amp_y" => set!(&params.trans_amp_y, v),
                "trans_amp_z" => set!(&params.trans_amp_z, v),
                "trans_mod_x" => set!(&params.trans_mod_x, v),
                "trans_mod_y" => set!(&params.trans_mod_y, v),
                "trans_mod_z" => set!(&params.trans_mod_z, v),
                "scale_amp" => set!(&params.scale_amp, v),
                "ambient" => set!(&params.ambient, v),
                "key_intensity" => set!(&params.key_intensity, v),
                "fill_intensity" => set!(&params.fill_intensity, v),
                "elevation" => set!(&params.elevation, v),
                "azimuth" => set!(&params.azimuth, v),
                "glow" => set!(&params.glow, v),
                "opacity" => set!(&params.opacity, v),
                "metallic" => set!(&params.metallic, v),
                "roughness" => set!(&params.roughness, v),
                "exposure" => set!(&params.exposure, v),
                "env_intensity" => set!(&params.env_intensity, v),
                "env_rotation" => set!(&params.env_rotation, v),
                "bloom_intensity" => set!(&params.bloom_intensity, v),
                "bloom_threshold" => set!(&params.bloom_threshold, v),
                "ior" => set!(&params.ior, v),
                "subsurface" => set!(&params.subsurface, v),
                "sss_distortion" => set!(&params.sss_distortion, v),
                "sss_power" => set!(&params.sss_power, v),
                "iridescence" => set!(&params.iridescence, v),
                "irid_scale" => set!(&params.irid_scale, v),
                "irid_shift" => set!(&params.irid_shift, v),
                "cam_speed" => set!(&params.cam_speed, v),
                "cam_kick" => set!(&params.cam_kick, v),
                "cam_damping" => set!(&params.cam_damping, v),
                "tempo" => set!(&params.tempo, v),
                // #317 levers: the camera-orbit path (enum), a colour hue, and the Harmonic
                // soft-body bell (a bool set from 0/1).
                "cam_path" => set!(&params.cam_path, HostCamPath::from_index(v as usize)),
                "mat_hue" => set!(&params.mat_hue, v),
                "bell_physical" => set!(&params.bell_physical, v > 0.5),
                _ => {}
            }
        }
    }
}

/// Drain the agent UI-sync apply channel each editor frame and mirror any NEW ops onto the
/// params (#317). Append-and-drain via a consumed-line cursor in `state`, so a param the
/// user moves after the agent set it is not re-applied (last-touched-wins). Seeded on first
/// drain so a prior session's lines aren't replayed. Only runs while the editor is open —
/// which is always the case when the agent is used, since the chat box lives in the editor.
fn agent_apply_drain(
    state: &mut preset::PresetUi,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    user_editing: bool,
) {
    // Last-touched-wins vs a live gesture: while the user is actively dragging/clicking a
    // control, defer the drain (don't consume the cursor) so a pending agent op can't
    // overwrite a slider mid-nudge. The ops apply on the next idle frame; a param the user
    // then moves after that sticks (the op is consumed and never replayed).
    if user_editing {
        return;
    }
    // Read once (missing file → empty, so the cursor still seeds on the first frame — the
    // first action then creates the file and IS applied next frame, never seeded over).
    let body = std::fs::read_to_string(ipc::agent_apply_path()).unwrap_or_default();
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let (range, seeded, cursor) =
        agent::apply_drain_plan(lines.len(), state.agent_apply_seeded, state.agent_apply_cursor);
    state.agent_apply_seeded = seeded;
    state.agent_apply_cursor = cursor;
    for line in &lines[range] {
        if let Some(op) = agent::ApplyOp::parse(line) {
            apply_agent_change(params, setter, &op);
        }
    }
}

fn perf_controller_drain(
    state: &mut preset::PresetUi,
    mailbox: &controller::Mailbox,
    apply_gen: &AtomicU32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    hdr_gen: &Arc<AtomicU32>,
    beat_pos: &Arc<AtomicU32>,
    now_t: f64,
) {
    use controller::{ArrowDir, ControllerEvent, RawKind};

    // Lazy-load the device profile once. Then, on the first frame AND after any
    // large wall-clock gap (the editor was closed while the audio thread kept
    // filling the mailbox), drain it so stale presses can't fire on reopen —
    // `perf_loaded` alone isn't enough because it never resets across reopens.
    let gap = now_t - state.perf_last_frame_time;
    state.perf_last_frame_time = now_t;
    // A gap > 0.5s (editor was closed while the clock ran on) OR a negative gap
    // (the clock reset on reopen) both mean a reopen — drain either way.
    if !state.perf_loaded {
        state.perf_layout = controller::load();
        state.perf_loaded = true;
        mailbox.drain();
    } else if gap > 0.5 || gap < 0.0 {
        mailbox.drain();
    }
    // #448: the knob bank's config loads beside the pad profile.
    if !state.knob_loaded {
        state.knob_config = controller::load_knobs();
        state.knob_loaded = true;
    }

    // Reconcile queued → active: a queued pad becomes active once its recall fires
    // (the presets rail's poll clears `pending_recall`). `perf_queued` is kept in
    // lock-step with `pending_recall` by `enqueue_recall` (which clears it on every
    // new recall), so a cleared `pending_recall` here always means THIS pad fired —
    // no cross-enum index compare, no risk of promoting a superseded pad.
    if let Some((comp, abs)) = state.perf_queued {
        if state.pending_recall.is_none() {
            state.perf_active[comp] = Some(abs);
            state.perf_queued = None;
        }
    }

    let enabled = params.perf_enable.value();
    while let Some(raw) = mailbox.pop() {
        state.perf_last_raw = Some(raw);
        if !enabled {
            continue;
        }

        // Learn capture: press pads in reading order across the whole 8×8 (top
        // row left→right, then down) — how people naturally do it, and matching
        // the mirror grid. Each reading-order press maps to its quadrant + slot
        // via the SAME layout the grid uses, so the display lines up with the
        // device. Note-off during learn is ignored.
        if let Some(idx) = state.perf_learn {
            if raw.kind == RawKind::NoteOn {
                let (comp, slot) = grid_pos_to_component_slot(idx);
                state.perf_layout.pads[comp.index()][slot as usize] = raw.data1;
                state.perf_layout.channel = raw.channel; // adopt the surface's channel
                state.perf_dirty = true;
                let next = idx + 1;
                state.perf_learn = if next >= 64 { None } else { Some(next) };
            }
            continue; // while learning, don't also recall
        }

        // #448: the rotary knob layer. Knob learn first (the twist-in-order
        // walk), then claim arbitration — a CC the knob bank owns drives its
        // target param directly and never reaches the pad router below.
        if raw.kind == RawKind::Cc {
            if let Some(idx) = state.knob_learn {
                let next = state.knob_config.layout.learn_capture(idx, raw);
                if next != idx {
                    state.knob_dirty = true;
                    state.knob_learn = if next >= controller::KNOB_COUNT {
                        state.perf_last_action =
                            Some("knob learn complete — 24 CCs captured".to_string());
                        None
                    } else {
                        Some(next)
                    };
                    // A re-captured layout re-engages softly (pickup restarts).
                    state.knob_engaged = [false; controller::KNOB_COUNT];
                    state.knob_last_cc = [None; controller::KNOB_COUNT];
                }
                continue; // while learning, knobs never actuate
            }
            if let Some(k) =
                controller::knob_claims(&state.knob_config.layout, &state.perf_layout, raw)
            {
                knob_apply(state, params, setter, k, raw.data2 as f32 / 127.0);
                continue;
            }
        }

        let Some(ev) = state.perf_layout.route(raw) else {
            continue;
        };
        match ev {
            ControllerEvent::Pad { component, slot, .. } => {
                let tab = component.editor_tab();
                let idx = state.perf_bank * 16 + slot as usize;
                // Clone name + values out of the immutable borrow before enqueue.
                let hit = state
                    .tab_presets
                    .get(tab.index())
                    .and_then(|v| v.get(idx))
                    .map(|p| (p.name.clone(), p.values.clone()));
                if let Some((name, values)) = hit {
                    let outcome = enqueue_recall(
                        state,
                        preset::PresetScope::Tab(tab),
                        values,
                        apply_gen,
                        params,
                        setter,
                        hdr_gen,
                        beat_pos,
                    );
                    let verb = match outcome {
                        RecallOutcome::Applied => "recalled",
                        RecallOutcome::Queued => "queued",
                    };
                    // Names it as a COMPONENT recall (never a Scene) so the behaviour
                    // is legible: only this component's params changed.
                    state.perf_last_action = Some(format!(
                        "{verb} {} slot {} — \"{name}\"",
                        component.label(),
                        slot + 1
                    ));
                    // Track the ABSOLUTE preset index so the grid highlight follows
                    // the preset across bank pages. (`enqueue_recall` already cleared
                    // `perf_queued`; re-set it only if this recall was deferred.)
                    match outcome {
                        RecallOutcome::Applied => {
                            state.perf_active[component.index()] = Some(idx);
                        }
                        RecallOutcome::Queued => {
                            state.perf_queued = Some((component.index(), idx));
                        }
                    }
                } else {
                    // Empty slot → no-op; report it so it doesn't read as "broken".
                    state.perf_last_action = Some(format!(
                        "{} slot {} — empty (no preset)",
                        component.label(),
                        slot + 1
                    ));
                }
            }
            ControllerEvent::Scene { slot } => {
                let idx = state.perf_bank * 16 + slot as usize;
                let hit = state.presets.get(idx).map(|p| (p.name.clone(), p.values.clone()));
                if let Some((name, values)) = hit {
                    enqueue_recall(
                        state,
                        preset::PresetScope::Global,
                        values,
                        apply_gen,
                        params,
                        setter,
                        hdr_gen,
                        beat_pos,
                    );
                    state.perf_last_action =
                        Some(format!("recalled SCENE slot {} — \"{name}\"", slot + 1));
                } else {
                    state.perf_last_action =
                        Some(format!("scene slot {} — empty (no preset)", slot + 1));
                }
            }
            ControllerEvent::Arrow(dir) => match dir {
                ArrowDir::Left => {
                    state.perf_bank = state.perf_bank.saturating_sub(1);
                    state.perf_last_action = Some(format!("bank ◀ → {}", state.perf_bank + 1));
                }
                ArrowDir::Right => {
                    state.perf_bank = (state.perf_bank + 1).min(MAX_PERF_BANK);
                    state.perf_last_action = Some(format!("bank ▶ → {}", state.perf_bank + 1));
                }
                ArrowDir::Up => {
                    step_component_division(params, setter, 1);
                    state.perf_last_action = Some("component timing ▲".to_string());
                }
                ArrowDir::Down => {
                    step_component_division(params, setter, -1);
                    state.perf_last_action = Some("component timing ▼".to_string());
                }
            },
            ControllerEvent::Function { pressed } => {
                if pressed {
                    // Cancel a pending quantized recall (panic / hold).
                    state.pending_recall = None;
                    state.perf_queued = None;
                    state.perf_last_action = Some("cancelled pending recall".to_string());
                }
            }
        }
    }

    if state.perf_dirty {
        state.perf_layout.save();
        state.perf_dirty = false;
    }
    if state.knob_dirty {
        state.knob_config.save();
        state.knob_dirty = false;
    }
}

/// #448: the knob bank's context key. Pickup engagement is per-context, so a
/// re-pointed bank (tab/generator/page switch) re-engages softly instead of
/// jumping params to wherever the knobs were last left.
fn knob_context_key(state: &preset::PresetUi, params: &OrganicMathParams) -> String {
    match state.knob_config.mode {
        controller::KnobMode::Performer => format!("perf:{}", state.knob_config.active_page),
        controller::KnobMode::Explore => {
            match controller::explore_knob_context(state.tab, params.generator.value().core()) {
                // A range key is its first anchor — distinct per generator.
                controller::KnobContext::Range(first, _) => format!("range:{first}"),
                controller::KnobContext::List(_) => format!("tab:{:?}", state.tab),
            }
        }
    }
}

/// #448: the declaration-ordered param universe for knob targeting, built
/// lazily ONCE from `Params::param_map()` (whose order is the declaration
/// order — the same contract the layout goldens in `param_table.rs` pin).
/// `ParamPtr`s are stable for the life of the plugin instance.
fn knob_param_universe<'a>(
    state: &'a mut preset::PresetUi,
    params: &OrganicMathParams,
) -> &'a (Vec<(String, ParamPtr)>, HashMap<String, usize>) {
    if state.knob_params.is_none() {
        let list: Vec<(String, ParamPtr)> = params
            .param_map()
            .into_iter()
            .map(|(id, ptr, _group)| (id, ptr))
            .collect();
        let index: HashMap<String, usize> = list
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.clone(), i))
            .collect();
        state.knob_params = Some((list, index));
    }
    state.knob_params.as_ref().unwrap()
}

/// #448: resolve the 24 knob targets for the current context — Explore follows
/// the editor's focus (generator block / curated tab bank), Performer reads the
/// active page's hand-assigned param IDs. Row-major, like the hardware.
fn knob_targets(
    state: &mut preset::PresetUi,
    params: &OrganicMathParams,
) -> [Option<ParamPtr>; controller::KNOB_COUNT] {
    let mode = state.knob_config.mode;
    let page_slots = state.knob_config.page().slots.clone();
    let tab = state.tab;
    let (list, index) = knob_param_universe(state, params);
    let mut out = [None; controller::KNOB_COUNT];
    match mode {
        controller::KnobMode::Performer => {
            for (i, slot) in page_slots.iter().enumerate() {
                if let Some(id) = slot {
                    out[i] = index.get(id).map(|&j| list[j].1);
                }
            }
        }
        controller::KnobMode::Explore => {
            match controller::explore_knob_context(tab, params.generator.value().core()) {
                controller::KnobContext::Range(first, end) => {
                    if let Some(&start) = index.get(first) {
                        let stop =
                            end.and_then(|e| index.get(e).copied()).unwrap_or(list.len());
                        for (i, j) in (start..stop).take(controller::KNOB_COUNT).enumerate() {
                            out[i] = Some(list[j].1);
                        }
                    }
                }
                controller::KnobContext::List(ids) => {
                    for (i, id) in ids.iter().enumerate() {
                        out[i] = index.get(*id).map(|&j| list[j].1);
                    }
                }
            }
        }
    }
    out
}

/// #448: one incoming knob message → its target param, through pickup, set via
/// the raw `GuiContext`. This is a REAL host param set (the same path the
/// sliders use), so the editor follows, presets capture it, and hosts record
/// it as automation — no override lane.
fn knob_apply(
    state: &mut preset::PresetUi,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    k: usize,
    cc_norm: f32,
) {
    let key = knob_context_key(state, params);
    if state.knob_context_key.as_deref() != Some(key.as_str()) {
        state.knob_engaged = [false; controller::KNOB_COUNT];
        state.knob_last_cc = [None; controller::KNOB_COUNT];
        state.knob_context_key = Some(key);
    }
    let Some(ptr) = knob_targets(state, params)[k] else {
        state.perf_last_action = Some(format!("knob {} — unmapped in this context", k + 1));
        return;
    };
    let current = unsafe { ptr.modulated_normalized_value() };
    let engaged = controller::pickup_engaged(
        state.knob_config.pickup,
        state.knob_engaged[k],
        state.knob_last_cc[k],
        cc_norm,
        current,
    );
    state.knob_last_cc[k] = Some(cc_norm);
    if engaged {
        state.knob_engaged[k] = true;
        unsafe {
            setter.raw_context.raw_begin_set_parameter(ptr);
            setter.raw_context.raw_set_parameter_normalized(ptr, cc_norm);
            setter.raw_context.raw_end_set_parameter(ptr);
        }
        state.perf_last_action = Some(format!("knob {} → {} = {}", k + 1, unsafe { ptr.name() }, unsafe {
            ptr.normalized_value_to_string(cc_norm, true)
        }));
    } else {
        state.perf_last_action = Some(format!(
            "knob {} waiting for pickup — {} is at {}",
            k + 1,
            unsafe { ptr.name() },
            unsafe { ptr.normalized_value_to_string(current, true) }
        ));
    }
}

/// Map a reading-order 8×8 grid position (0..64, row-major from the TOP-left) to
/// its quadrant component + slot (slot 0 = the quadrant's bottom-left pad). The
/// single source of truth shared by the mirror grid and the learn flow, so the
/// on-screen display always matches how a captured pad routes.
fn grid_pos_to_component_slot(idx: usize) -> (controller::Component, u8) {
    use controller::Component;
    let row = idx / 8;
    let col = idx % 8;
    let comp = match (row < 4, col < 4) {
        (true, true) => Component::Generator,     // top-left (pink)
        (true, false) => Component::Look,         // top-right (green)
        (false, true) => Component::Motion,       // bottom-left (yellow)
        (false, false) => Component::Environment, // bottom-right (blue)
    };
    let qr = row % 4; // 0 = top row of the quadrant
    let lc = col % 4;
    let slot = ((3 - qr) * 4 + lc) as u8; // slot 0 = quadrant bottom-left
    (comp, slot)
}

/// Draw the 8×8 mirror grid: each quadrant tinted its factory colour, cells dim
/// (has-preset) / bright (active) / pulsing (queued-until-boundary).
fn draw_perf_grid(ui: &mut egui::Ui, state: &preset::PresetUi) {
    use controller::Component;
    let n = 8usize;
    let avail = ui.available_width().min(280.0).max(64.0);
    let cell = (avail / n as f32).floor().max(8.0);
    let size = cell * n as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let time = ui.input(|i| i.time);
    let pulse = 0.5 + 0.5 * (time * 6.0).sin() as f32;

    let comp_color = |c: Component| match c {
        Component::Generator => egui::Color32::from_rgb(230, 90, 170), // pink
        Component::Motion => egui::Color32::from_rgb(220, 200, 70),    // yellow
        Component::Look => egui::Color32::from_rgb(90, 200, 110),      // green
        Component::Environment => egui::Color32::from_rgb(80, 140, 230), // blue
    };

    for row in 0..n {
        for col in 0..n {
            // Same reading-order mapping the learn flow captures with, so the grid
            // display matches the physical device.
            let (comp, slot) = grid_pos_to_component_slot(row * n + col);
            let idx = state.perf_bank * 16 + slot as usize;
            let has = state
                .tab_presets
                .get(comp.editor_tab().index())
                .and_then(|v| v.get(idx))
                .is_some();
            // Compare the cell's ABSOLUTE index so the highlight tracks the actual
            // preset, not a bank-local slot (else paging banks lights the wrong pad).
            let active = state.perf_active[comp.index()] == Some(idx);
            let queued = state.perf_queued == Some((comp.index(), idx));

            let base = comp_color(comp);
            // Every pad is drawn tinted its quadrant colour so the surface always
            // reads as a full 8×8 grid (like the lit device). Brightness layers on
            // top: empty (faint) → has-preset (mid) → active (full) / queued (pulse).
            let scale = |c: egui::Color32, f: f32| {
                egui::Color32::from_rgb(
                    (c.r() as f32 * f) as u8,
                    (c.g() as f32 * f) as u8,
                    (c.b() as f32 * f) as u8,
                )
            };
            let empty = scale(base, 0.18);
            let has_col = scale(base, 0.5);
            let mut fill = if active {
                base
            } else if has {
                has_col
            } else {
                empty
            };
            if queued {
                let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * pulse) as u8;
                fill = egui::Color32::from_rgb(
                    lerp(has_col.r(), base.r()),
                    lerp(has_col.g(), base.g()),
                    lerp(has_col.b(), base.b()),
                );
            }
            let cell_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + col as f32 * cell, rect.min.y + row as f32 * cell),
                egui::vec2(cell - 1.5, cell - 1.5),
            );
            painter.rect_filled(cell_rect, egui::CornerRadius::same(2), fill);
        }
    }
}

/// #448: the 3×8 knob mirror — each cell shows its target param + live value;
/// engaged knobs read bright (accent). In Performer mode a click on a cell
/// opens that slot's binding picker.
fn draw_knob_grid(ui: &mut egui::Ui, state: &mut preset::PresetUi, params: &OrganicMathParams) {
    use controller::{KNOB_COLS, KNOB_COUNT};
    let targets = knob_targets(state, params);
    let performer = state.knob_config.mode == controller::KnobMode::Performer;
    let cols = KNOB_COLS;
    let rows = KNOB_COUNT / cols;
    let avail = ui.available_width().clamp(240.0, 560.0);
    let cell_w = (avail / cols as f32).floor();
    let cell_h = 30.0;
    let size = egui::vec2(cell_w * cols as f32, cell_h * rows as f32);
    let sense = if performer { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter_at(rect);
    let clicked_cell = resp
        .clicked()
        .then(|| resp.interact_pointer_pos())
        .flatten()
        .map(|p| {
            let col = (((p.x - rect.min.x) / cell_w) as usize).min(cols - 1);
            let row = (((p.y - rect.min.y) / cell_h) as usize).min(rows - 1);
            row * cols + col
        });
    // Rough per-cell text budget so long param names don't bleed into the
    // neighbour cell (the painter clips to the GRID, not the cell).
    let max_chars = ((cell_w - 6.0) / 5.0).max(4.0) as usize;
    for row in 0..rows {
        for col in 0..cols {
            let k = row * cols + col;
            let cell = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + col as f32 * cell_w, rect.min.y + row as f32 * cell_h),
                egui::vec2(cell_w - 2.0, cell_h - 2.0),
            );
            painter.rect_filled(cell, egui::CornerRadius::same(3), ui.visuals().extreme_bg_color);
            let (label, value) = match targets[k] {
                Some(ptr) => unsafe {
                    (ptr.name().to_string(), Some(ptr.modulated_normalized_value()))
                },
                None => ("—".to_string(), None),
            };
            let label = if label.chars().count() > max_chars {
                let mut s: String = label.chars().take(max_chars.saturating_sub(1)).collect();
                s.push('…');
                s
            } else {
                label
            };
            let text_color = if state.knob_engaged[k] {
                ACCENT()
            } else {
                ui.visuals().weak_text_color()
            };
            painter.text(
                cell.min + egui::vec2(3.0, 3.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(9.0),
                text_color,
            );
            // Live value bar along the cell's bottom edge.
            let bar = egui::Rect::from_min_size(
                egui::pos2(cell.min.x + 3.0, cell.max.y - 7.0),
                egui::vec2(cell.width() - 6.0, 4.0),
            );
            painter.rect_filled(bar, egui::CornerRadius::same(1), ui.visuals().faint_bg_color);
            if let Some(v) = value {
                let mut fg = bar;
                fg.set_width(bar.width() * v.clamp(0.0, 1.0));
                let bar_col = if state.knob_engaged[k] {
                    ACCENT()
                } else {
                    ui.visuals().strong_text_color()
                };
                painter.rect_filled(fg, egui::CornerRadius::same(1), bar_col);
            }
        }
    }
    if let Some(k) = clicked_cell {
        state.knob_assign_open = Some(k);
        state.knob_filter.clear();
    }
}

/// #448: the rotary-knob bank section of the Performance Controller window —
/// mode + pickup, Performer pages, the 3×8 mirror grid, the slot-binding
/// picker, and the twist-in-order learn flow.
fn knob_bank_section(ui: &mut egui::Ui, state: &mut preset::PresetUi, params: &OrganicMathParams) {
    use controller::{KnobMode, KNOB_COLS};
    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new("🎛 Rotary knobs — Launch Control XL").strong());
    help(
        ui,
        "24 encoders (3 rows of 8) drive PARAMS the way the pads drive presets. \
         Explore follows the editor's focus — the selected generator's dials on \
         the Generator tab, curated Motion / Look / Environment banks on those \
         tabs — numbered onto the knobs row-major. Performer is a hand-assigned \
         page of 24 bindings (Ableton-macro style): click a cell to bind any \
         param, keep named pages per set. Pickup = soft takeover: a knob only \
         engages once it meets the param's current value, so preset recalls \
         never make a knob jump. Default profile: Launch Control XL factory \
         rows — use Learn to capture your device (twist all 24 in order).",
    );
    ui.horizontal(|ui| {
        let mut mode = state.knob_config.mode;
        ui.selectable_value(&mut mode, KnobMode::Explore, "Explore")
            .on_hover_text("Context-aware: the bank follows the focused tab / generator");
        ui.selectable_value(&mut mode, KnobMode::Performer, "Performer")
            .on_hover_text("A hand-assigned page of 24 param bindings");
        if mode != state.knob_config.mode {
            state.knob_config.mode = mode;
            state.knob_context_key = None; // re-engage softly in the new context
            state.knob_assign_open = None;
            state.knob_dirty = true;
        }
        let mut pickup = state.knob_config.pickup;
        if ui
            .checkbox(&mut pickup, "pickup")
            .on_hover_text(
                "Soft takeover — a knob engages only when it reaches the param's current value",
            )
            .changed()
        {
            state.knob_config.pickup = pickup;
            state.knob_dirty = true;
        }
    });

    if state.knob_config.mode == KnobMode::Performer {
        ui.horizontal(|ui| {
            ui.label("page:");
            let mut switch: Option<usize> = None;
            egui::ComboBox::from_id_salt("knob_page")
                .selected_text(state.knob_config.page().name.clone())
                .show_ui(ui, |ui| {
                    for (i, p) in state.knob_config.pages.iter().enumerate() {
                        if ui
                            .selectable_label(i == state.knob_config.active_page, &p.name)
                            .clicked()
                        {
                            switch = Some(i);
                        }
                    }
                });
            if let Some(i) = switch {
                if i != state.knob_config.active_page {
                    state.knob_config.active_page = i;
                    state.knob_context_key = None;
                    state.knob_assign_open = None;
                    state.knob_dirty = true;
                }
            }
            if ui.small_button("＋").on_hover_text("New empty page").clicked() {
                let n = state.knob_config.pages.len() + 1;
                state
                    .knob_config
                    .pages
                    .push(controller::KnobPage::new(&format!("Page {n}")));
                state.knob_config.active_page = state.knob_config.pages.len() - 1;
                state.knob_context_key = None;
                state.knob_dirty = true;
            }
            let mut name = state.knob_config.page().name.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut name).desired_width(110.0))
                .changed()
            {
                state.knob_config.page_mut().name = name;
                state.knob_dirty = true;
            }
        });
        ui.label(
            egui::RichText::new("click a cell below to bind that knob").weak().small(),
        );
    } else {
        let what = match state.tab {
            preset::UiTab::Motion => "the Motion bank (clock / pulse / routing / camera)".to_string(),
            preset::UiTab::Look => "the Look bank (material / colour / FX)".to_string(),
            preset::UiTab::Environment => {
                "the Environment bank (IBL / backdrop / world layers)".to_string()
            }
            preset::UiTab::Synth => "the Synth engine (the Sound card)".to_string(),
            _ => format!("{} — its generator dials, in card order", params.generator),
        };
        ui.label(egui::RichText::new(format!("knobs → {what}")).weak().small());
    }

    ui.add_space(2.0);
    draw_knob_grid(ui, state, params);

    // Performer slot-binding picker (opened by clicking a grid cell).
    if state.knob_config.mode == KnobMode::Performer {
        if let Some(slot) = state.knob_assign_open {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "bind knob {} (row {}, knob {}):",
                        slot + 1,
                        slot / KNOB_COLS + 1,
                        slot % KNOB_COLS + 1
                    ))
                    .strong(),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut state.knob_filter)
                        .hint_text("filter params…")
                        .desired_width(140.0),
                );
                if ui.small_button("✖ unbind").clicked() {
                    state.knob_config.page_mut().slots[slot] = None;
                    state.knob_dirty = true;
                    state.knob_assign_open = None;
                }
                if ui.small_button("close").clicked() {
                    state.knob_assign_open = None;
                }
            });
            // Snapshot the matches first — the universe borrow must end before
            // we mutate the config on a click.
            let filter = state.knob_filter.to_lowercase();
            let matches: Vec<(String, String)> = {
                let (list, _) = knob_param_universe(state, params);
                list.iter()
                    .filter_map(|(id, ptr)| {
                        let name = unsafe { ptr.name() }.to_string();
                        (filter.is_empty()
                            || name.to_lowercase().contains(&filter)
                            || id.contains(&filter))
                        .then(|| (id.clone(), name))
                    })
                    .take(60)
                    .collect()
            };
            egui::ScrollArea::vertical()
                .id_salt("knob_assign")
                .max_height(140.0)
                .show(ui, |ui| {
                    for (id, name) in matches {
                        if ui
                            .selectable_label(false, format!("{name}   [{id}]"))
                            .clicked()
                        {
                            state.knob_config.page_mut().slots[slot] = Some(id);
                            state.knob_dirty = true;
                            state.knob_assign_open = None;
                        }
                    }
                });
        }
    }

    // Learn flow — same shape as the pads, but capture is BY TWISTING, in
    // row-major order (an encoder streams repeats; only a NEW CC advances).
    ui.add_space(4.0);
    ui.horizontal(|ui| match state.knob_learn {
        None => {
            if ui
                .button("🎓 Learn knobs")
                .on_hover_text(
                    "Twist all 24 knobs in ORDER — top row left-to-right, then the \
                     middle row, then the bottom row — to capture your device's CCs \
                     (and its channel, which un-collides them from the Launchpad).",
                )
                .clicked()
            {
                state.knob_learn = Some(0);
                state.perf_learn = None; // one learn flow at a time
            }
            if ui
                .button("⟲ Reset to Launch Control XL")
                .on_hover_text("Restore the factory-template rows (CC 13–20 / 29–36 / 49–56)")
                .clicked()
            {
                state.knob_config.layout = controller::KnobLayout::launch_control_xl();
                state.knob_dirty = true;
            }
        }
        Some(idx) => {
            ui.label(
                egui::RichText::new(format!(
                    "Twist knob {}/24 — row {}, knob {}",
                    idx + 1,
                    idx / KNOB_COLS + 1,
                    idx % KNOB_COLS + 1
                ))
                .strong(),
            );
            if ui.button("Cancel").clicked() {
                state.knob_learn = None;
            }
        }
    });
}

/// #356: the Performance Controller window body — enable toggle, timing, the
/// mirror grid, the learn flow, and a live MIDI diagnostic readout.
fn perf_controller_card(
    ui: &mut egui::Ui,
    state: &mut preset::PresetUi,
    params: &OrganicMathParams,
    setter: &ParamSetter,
) {
    crow(ui, "enable performance controller", &params.perf_enable, setter);
    help(
        ui,
        "Maps a Launchpad-style 8×8 pad grid to the four Scene components — each \
         quadrant recalls one component's presets (Generator top-left, Motion \
         bottom-left, Look top-right, Environment bottom-right), beat-quantized on \
         the Component-timing division. Off = the pad surface is ignored (Key Map / \
         synth untouched). Default profile: Novation Launchpad Mini MK3 — use Learn \
         to capture your own device.",
    );

    let enabled = params.perf_enable.value();
    ui.add_enabled_ui(enabled, |ui| {
        let avail = ui.available_width();
        param_combo(ui, avail, "component timing", &params.component_preset_timing, setter);
        param_combo(ui, avail, "scene timing", &params.scene_preset_timing, setter);

        ui.horizontal(|ui| {
            ui.label(format!("Bank {}", state.perf_bank + 1));
            if ui.small_button("◀").on_hover_text("Previous bank of 16").clicked() {
                state.perf_bank = state.perf_bank.saturating_sub(1);
            }
            if ui.small_button("▶").on_hover_text("Next bank of 16").clicked() {
                state.perf_bank = (state.perf_bank + 1).min(MAX_PERF_BANK);
            }
            if ui.small_button("✖ Cancel pending").on_hover_text("Cancel a queued recall").clicked() {
                state.pending_recall = None;
                state.perf_queued = None;
            }
        });

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Surface — bright = active, pulsing = queued").weak().small());
        draw_perf_grid(ui, state);

        // Learn flow (#356 §7): walk all 64 pads, capturing each note number.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            match state.perf_learn {
                None => {
                    if ui
                        .button("🎓 Learn pads")
                        .on_hover_text(
                            "Press all 64 pads in READING ORDER — top row left-to-right, \
                             then the next row down — to capture your device's layout.",
                        )
                        .clicked()
                    {
                        state.perf_learn = Some(0);
                        state.knob_learn = None; // one learn flow at a time
                    }
                    if ui
                        .button("⟲ Reset to Mini MK3")
                        .on_hover_text("Restore the default Launchpad Mini MK3 profile")
                        .clicked()
                    {
                        state.perf_layout = controller::PadLayout::mini_mk3();
                        state.perf_dirty = true;
                    }
                }
                Some(idx) => {
                    let (comp, slot) = grid_pos_to_component_slot(idx);
                    let row = idx / 8;
                    let col = idx % 8;
                    ui.label(
                        egui::RichText::new(format!(
                            "Press pad {}/64 — row {}, col {} (→ {} slot {})",
                            idx + 1,
                            row + 1,
                            col + 1,
                            comp.label(),
                            slot + 1
                        ))
                        .strong(),
                    );
                    if ui.button("Cancel").clicked() {
                        state.perf_learn = None;
                    }
                }
            }
        });

        // #448: the rotary knob bank rides the same enable gate — one
        // performance surface, two devices (pads recall, knobs sculpt).
        knob_bank_section(ui, state, params);
    });

    // Live diagnostic (#356 §7) — OUTSIDE the enabled-gate so it stays readable
    // when the controller is OFF, making it a true "is MIDI arriving?" probe:
    // raw MIDI is mirrored here regardless of the enable state. The action line
    // names the last acted gesture as a COMPONENT recall (never a Scene).
    ui.separator();
    if let Some(action) = &state.perf_last_action {
        ui.label(egui::RichText::new(format!("↳ {action}")).color(ACCENT()).small());
    }
    let diag = match state.perf_last_raw {
        Some(raw) => {
            let route = state
                .perf_layout
                .route(raw)
                .map(|e| format!("{e:?}"))
                .unwrap_or_else(|| "—".to_string());
            format!(
                "last: {:?} ch{} d1={} d2={}  →  {route}",
                raw.kind, raw.channel, raw.data1, raw.data2
            )
        }
        None => "last: (no MIDI received — check the plugin's MIDI routing)".to_string(),
    };
    ui.label(egui::RichText::new(diag).weak().small().monospace());
}

/// Reset every parameter to its default — reuses the preset machinery by applying
/// a freshly-defaulted parameter set through the host setter. Uses `apply_all`
/// (every tab), NOT `apply()` (Scene-only), so Audio/Synth/Settings params reset
/// too — a genuine hard factory reset (#354).
fn reset_all(apply_gen: &AtomicU32, params: &OrganicMathParams, setter: &ParamSetter) {
    let defaults = preset::PresetValues::capture(&OrganicMathParams::default());
    apply_gen.fetch_add(1, Ordering::AcqRel); // → odd: applying
    defaults.apply_all(params, setter);
    apply_gen.fetch_add(1, Ordering::AcqRel); // → even: stable
}

// ---------------------------------------------------------------------------
// Audio Reactivity panel
// ---------------------------------------------------------------------------

/// dB-scaled meter normalization: maps ~[-60 dB, +6 dB] → [0, 1], so quiet
/// detail is visible and full-scale (≈1.0 linear) sits near the top.
fn meter_norm(v: f32) -> f32 {
    let db = 20.0 * v.max(1.0e-6).log10();
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

/// Linear interpolate two colours.
fn lerp_col(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    egui::Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Green → amber → red, by fill fraction (classic meter ramp).
fn meter_color(t: f32) -> egui::Color32 {
    let green = egui::Color32::from_rgb(80, 200, 110);
    let red = egui::Color32::from_rgb(235, 70, 55);
    if t < 0.7 {
        lerp_col(green, ACCENT(), t / 0.7)
    } else {
        lerp_col(ACCENT(), red, (t - 0.7) / 0.3)
    }
}

const METER_BG: egui::Color32 = egui::Color32::from_rgb(8, 10, 16);
const CAP_COL: egui::Color32 = egui::Color32::from_rgb(220, 225, 235);

/// Vertical bar meter with a height-graded gradient fill + peak-hold cap.
fn paint_vmeter(p: &egui::Painter, rect: egui::Rect, t: f32, peak: f32) {
    p.rect_filled(rect, egui::CornerRadius::same(3), METER_BG);
    let inner = rect.shrink(3.0);
    let segs = 24;
    for s in 0..segs {
        let f0 = s as f32 / segs as f32;
        if f0 >= t {
            break;
        }
        let f1 = ((s + 1) as f32 / segs as f32).min(t);
        let seg = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - inner.height() * f1),
            egui::pos2(inner.right(), inner.bottom() - inner.height() * f0),
        );
        p.rect_filled(seg, egui::CornerRadius::same(0), meter_color(f0));
    }
    if peak > 0.01 {
        let y = inner.bottom() - inner.height() * peak.clamp(0.0, 1.0);
        let cap = egui::Rect::from_min_max(
            egui::pos2(inner.left(), y - 1.5),
            egui::pos2(inner.right(), y),
        );
        p.rect_filled(cap, egui::CornerRadius::same(0), meter_color(peak));
    }
}

/// Spectrum analyzer: one bar per display bin, with a falling peak cap held in
/// `peaks` (GUI-side decay, decoupled from the audio block rate).
fn paint_spectrum(
    p: &egui::Painter,
    rect: egui::Rect,
    spec: &[f32],
    peaks: &mut [f32],
    dt: f32,
) {
    p.rect_filled(rect, egui::CornerRadius::same(3), METER_BG);
    let inner = rect.shrink(3.0);
    let n = spec.len().max(1);
    let gap = 2.0;
    let bw = ((inner.width() - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
    const FALL: f32 = 0.9; // peak-cap fall, fraction/sec
    for i in 0..n {
        let t = meter_norm(spec[i]);
        peaks[i] = if t >= peaks[i] {
            t
        } else {
            (peaks[i] - FALL * dt).max(t).max(0.0)
        };
        let x0 = inner.left() + i as f32 * (bw + gap);
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0, inner.bottom() - inner.height() * t),
            egui::pos2(x0 + bw, inner.bottom()),
        );
        p.rect_filled(bar, egui::CornerRadius::same(1), meter_color(t));
        let py = inner.bottom() - inner.height() * peaks[i];
        let cap = egui::Rect::from_min_max(egui::pos2(x0, py - 1.5), egui::pos2(x0 + bw, py));
        p.rect_filled(cap, egui::CornerRadius::same(0), CAP_COL);
    }
}

/// A labelled horizontal band-level bar (one of the five routable sources).
fn band_bar(ui: &mut egui::Ui, label: &str, value: f32) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [62.0, 14.0],
            egui::Label::new(egui::RichText::new(label).monospace()),
        );
        let w = ui.available_width().max(40.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 14.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, egui::CornerRadius::same(3), METER_BG);
        let inner = rect.shrink(2.0);
        let t = meter_norm(value);
        let fill = egui::Rect::from_min_max(
            inner.min,
            egui::pos2(inner.left() + inner.width() * t, inner.bottom()),
        );
        p.rect_filled(fill, egui::CornerRadius::same(2), meter_color(t));
    });
}

// ---------------------------------------------------------------------------
// Performance / diagnostics status bar (#277)
// ---------------------------------------------------------------------------

/// Frame-time budget for a 60 fps target (ms). The status bar reads everything
/// against this so "how close are we to redline" is a single, honest ruler.
/// Height of the perf status strip (`TopBottomPanel::bottom("perf_status_bar")`).
/// Named so the #532 dock arithmetic sees the same vertical budget the user does.
const PERF_BAR_H: f32 = 100.0;

/// Floor for the resizable perf bar — enough that the dot, the FPS number and one meter
/// row stay legible, so dragging it down hides detail rather than producing a strip that
/// reads as a rendering fault. Well under [`PERF_BAR_H`], which remains the default.
const PERF_BAR_MIN_H: f32 = 48.0;

const PERF_BUDGET_MS: f32 = 1000.0 / 60.0;
/// How many frame-time samples the scrolling graph holds (a few seconds at 60).
const PERF_HIST_LEN: usize = 180;

/// Health colour for a frames-per-second readout: green with headroom, amber as
/// it sags toward 30, red below. (Distinct from `meter_color`, which ramps by
/// fill fraction — this ramps by the *value* so the number itself reads hot.)
fn fps_color(fps: f32) -> egui::Color32 {
    let green = egui::Color32::from_rgb(80, 200, 110);
    let red = egui::Color32::from_rgb(235, 70, 55);
    if fps >= 58.0 {
        green
    } else if fps >= 30.0 {
        // 58 → green, 30 → amber.
        lerp_col(ACCENT(), green, ((fps - 30.0) / 28.0).clamp(0.0, 1.0))
    } else {
        // 30 → amber, 12 → red.
        lerp_col(red, ACCENT(), ((fps - 12.0) / 18.0).clamp(0.0, 1.0))
    }
}

/// Horizontal "frame load" meter: `frame_ms` scaled so the 60 fps budget sits at
/// the half-way tick, drawn as the same segmented gradient the audio meters use,
/// with a bright budget tick at the middle. Fill past the tick = over budget.
fn paint_load_bar(p: &egui::Painter, rect: egui::Rect, frame_ms: f32) {
    p.rect_filled(rect, egui::CornerRadius::same(3), METER_BG);
    let inner = rect.shrink(3.0);
    // Full scale = 2× budget (≈30 fps at the right edge); budget is the midpoint.
    let frac = (frame_ms / (PERF_BUDGET_MS * 2.0)).clamp(0.0, 1.0);
    let segs = 40;
    for s in 0..segs {
        let f0 = s as f32 / segs as f32;
        if f0 >= frac {
            break;
        }
        let f1 = ((s + 1) as f32 / segs as f32).min(frac);
        let seg = egui::Rect::from_min_max(
            egui::pos2(inner.left() + inner.width() * f0, inner.top()),
            egui::pos2(inner.left() + inner.width() * f1, inner.bottom()),
        );
        p.rect_filled(seg, egui::CornerRadius::same(0), meter_color(f0));
    }
    // Budget tick at the midpoint (16.7 ms / 60 fps).
    let tx = inner.left() + inner.width() * 0.5;
    p.vline(
        tx,
        inner.top()..=inner.bottom(),
        egui::Stroke::new(1.5, CAP_COL),
    );
}

/// Scrolling frame-time history: one column per sample (oldest left, newest
/// right), height ∝ frame time, coloured by load, with faint guide lines at the
/// 60 fps and 30 fps frame times so the trend reads against the budget.
fn paint_frametime_graph(p: &egui::Painter, rect: egui::Rect, hist: &[f32]) {
    p.rect_filled(rect, egui::CornerRadius::same(3), METER_BG);
    let inner = rect.shrink(3.0);
    // Full scale = 3× budget (≈20 fps at the top) so the 60/30 lines sit high.
    let full = PERF_BUDGET_MS * 3.0;
    for (ms, col) in [
        (PERF_BUDGET_MS, egui::Color32::from_rgb(60, 110, 70)), // 60 fps
        (PERF_BUDGET_MS * 2.0, egui::Color32::from_rgb(120, 90, 40)), // 30 fps
    ] {
        let y = inner.bottom() - inner.height() * (ms / full).clamp(0.0, 1.0);
        p.hline(
            inner.left()..=inner.right(),
            y,
            egui::Stroke::new(1.0, col),
        );
    }
    let n = hist.len();
    if n == 0 {
        return;
    }
    let bw = (inner.width() / n as f32).max(0.5);
    for (i, &ms) in hist.iter().enumerate() {
        let t = (ms / full).clamp(0.0, 1.0);
        let x0 = inner.left() + i as f32 * bw;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x0, inner.bottom() - inner.height() * t),
            egui::pos2(x0 + bw, inner.bottom()),
        );
        // Colour by load against the budget (t is on a 0..3× scale, so ×1.5
        // maps budget→~0.5 amber, 2× budget→~1.0 red).
        p.rect_filled(bar, egui::CornerRadius::same(0), meter_color((t * 1.5).min(1.0)));
    }
}

/// A compact diagnostics tile (#277 Tier 2): a small label, a value (coloured by
/// load), and a thin load-coloured meter. `frac` is the 0..1 headroom-used fill —
/// green with headroom, red at/over the ceiling.
fn stat_tile(ui: &mut egui::Ui, w: f32, label: &str, value: String, frac: f32) {
    let f = frac.clamp(0.0, 1.0);
    ui.allocate_ui(egui::vec2(w, 40.0), |ui| {
        ui.set_width(w);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).small().weak());
            ui.label(
                egui::RichText::new(value)
                    .monospace()
                    .strong()
                    .color(meter_color(f)),
            );
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 6.0), egui::Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, egui::CornerRadius::same(2), METER_BG);
            let inner = rect.shrink(1.0);
            let fill = egui::Rect::from_min_max(
                inner.min,
                egui::pos2(inner.left() + inner.width() * f, inner.bottom()),
            );
            p.rect_filled(fill, egui::CornerRadius::same(1), meter_color(f));
        });
    });
}

/// Human-readable count with a k/M suffix (for the instance tile).
fn fmt_count(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f32 / 1.0e6)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f32 / 1.0e3)
    } else {
        n.to_string()
    }
}

/// The bottom performance / diagnostics status bar (#277). Tier 1 = the always-on
/// frame rate + frame-load meter + scrolling frame-time graph; Tier 2 adds the
/// workload tiles (CPU ms, instances, render scale, fill, TLAS); Tier 3 adds the
/// GPU/CPU vertical hero meters + the GPU-ms tile (from timestamp queries). Read
/// from the visual's `Feedback` reverse channel; toggled by the 📊 Perf button.
fn perf_bar_ui(
    ui: &mut egui::Ui,
    feedback: Option<&crate::ipc::Feedback>,
    state: &mut preset::PresetUi,
    active_bpm: f32,
    tempo_src: u32,
) {
    let running = feedback.is_some();
    let fps = feedback.map(|f| f.fps).unwrap_or(0.0);
    let frame_ms = if fps > 0.1 { 1000.0 / fps } else { 0.0 };

    // Sample the reported frame time into the history ring on a steady ~60 Hz wall
    // clock, NOT once per repaint: the editor calls request_repaint() continuously,
    // so a per-repaint push would duplicate samples and warp the "last few seconds"
    // span whenever repaints outrun (or lag) the visual's frames. The visual already
    // smooths fps, so a fixed cadence scrolls smoothly and keeps the time axis honest.
    const PERF_SAMPLE_DT: f64 = 1.0 / 60.0;
    let now = ui.input(|i| i.time);
    if running && frame_ms > 0.0 && now >= state.perf_next_sample_t {
        // Re-anchor if we've fallen far behind (window hidden/paused) so we resume a
        // steady cadence instead of bursting a backlog of catch-up samples.
        state.perf_next_sample_t = if now - state.perf_next_sample_t > PERF_SAMPLE_DT {
            now + PERF_SAMPLE_DT
        } else {
            state.perf_next_sample_t + PERF_SAMPLE_DT
        };
        state.perf_hist.push(frame_ms);
        if state.perf_hist.len() > PERF_HIST_LEN {
            let over = state.perf_hist.len() - PERF_HIST_LEN;
            state.perf_hist.drain(0..over);
        }
    }

    ui.add_space(2.0);
    ui.horizontal_top(|ui| {
        // ── Left: the headline readouts ──────────────────────────────────
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let (dot, dcol) = if !running {
                    ("●", egui::Color32::from_rgb(110, 116, 130))
                } else if fps >= 30.0 {
                    ("●", egui::Color32::from_rgb(80, 200, 110))
                } else {
                    ("●", egui::Color32::from_rgb(235, 70, 55))
                };
                ui.label(egui::RichText::new(dot).color(dcol));
                ui.label(
                    egui::RichText::new("PERFORMANCE")
                        .small()
                        .weak()
                        .strong(),
                );
            });
            if running {
                ui.label(
                    egui::RichText::new(format!("{:.0}", fps))
                        .heading()
                        .strong()
                        .color(fps_color(fps)),
                );
                ui.label(egui::RichText::new(format!("fps · {:.1} ms", frame_ms)).small().weak());
            } else {
                ui.label(
                    egui::RichText::new("—")
                        .heading()
                        .strong()
                        .color(egui::Color32::from_rgb(110, 116, 130)),
                );
                ui.label(egui::RichText::new("visual not running").small().weak());
            }
            // Tempo readout (from the plugin's beat clock — always live, even with
            // the visual closed). Shows the active BPM + where it comes from, so you
            // can SEE the manual dial / host / audio-detect taking effect.
            let (src_name, src_col) = match tempo_src {
                1 => ("Audio-detect", egui::Color32::from_rgb(235, 170, 60)),
                2 => ("Manual", egui::Color32::from_rgb(90, 180, 235)),
                _ => ("Host-sync", egui::Color32::from_rgb(120, 200, 130)),
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("♩ {:.1} BPM", active_bpm)).strong());
                ui.label(egui::RichText::new(src_name).small().strong().color(src_col));
            });
            // Resolution / render-scale / output size.
            if let Some(f) = feedback {
                let scale = format!("{}%", (f.scale * 100.0).round() as i32);
                let base = format!("{}×{} {}", f.width, f.height, scale);
                let out = if f.out_w > 0 && f.out_h > 0 {
                    format!("  → out {}×{}", f.out_w, f.out_h)
                } else {
                    String::new()
                };
                ui.label(egui::RichText::new(format!("{base}{out}")).small().weak().monospace());
            }
        });

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);

        // ── GPU / CPU hero meters (Tier 3) — how close each is to redline ──
        // Two vertical meters (the audio IN-meter idiom): the budget sits at the
        // half-way cap tick, so a fill reaching the tick = at 60 fps budget, above
        // it = over. GPU reads "n/a" on a device without timestamp queries.
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("GPU / CPU").small().weak());
            ui.horizontal_top(|ui| {
                // GPU hero (the headroom centrepiece).
                ui.vertical(|ui| {
                    let (gtxt, gfrac, gcol) = match feedback {
                        Some(f) if f.gpu_timing_available != 0 => {
                            let frac = (f.gpu_ms / (PERF_BUDGET_MS * 2.0)).clamp(0.0, 1.0);
                            (format!("{:.1}", f.gpu_ms), frac, meter_color(frac))
                        }
                        _ => ("n/a".to_string(), 0.0, egui::Color32::from_rgb(110, 116, 130)),
                    };
                    ui.label(egui::RichText::new("GPU").small().weak());
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(24.0, 40.0), egui::Sense::hover());
                    paint_vmeter(&ui.painter_at(rect), rect, gfrac, 0.5);
                    ui.label(egui::RichText::new(gtxt).small().monospace().color(gcol));
                });
                ui.add_space(4.0);
                // CPU hero.
                ui.vertical(|ui| {
                    let cfrac = feedback
                        .map(|f| (f.cpu_ms / (PERF_BUDGET_MS * 2.0)).clamp(0.0, 1.0))
                        .unwrap_or(0.0);
                    let ctxt = feedback
                        .map(|f| format!("{:.1}", f.cpu_ms))
                        .unwrap_or_else(|| "—".to_string());
                    ui.label(egui::RichText::new("CPU").small().weak());
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(24.0, 40.0), egui::Sense::hover());
                    paint_vmeter(&ui.painter_at(rect), rect, cfrac, 0.5);
                    ui.label(egui::RichText::new(ctxt).small().monospace().color(meter_color(cfrac)));
                });
            });
        });

        ui.add_space(12.0);

        // ── Middle: the wall-clock frame-load meter + headroom ───────────
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("FRAME LOAD   ·   vs 16.7 ms (60 fps) budget")
                    .small()
                    .weak(),
            );
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(200.0, 20.0), egui::Sense::hover());
            paint_load_bar(&ui.painter_at(rect), rect, frame_ms);
            let headroom = if frame_ms > 0.0 {
                (1.0 - frame_ms / PERF_BUDGET_MS).clamp(-9.9, 1.0) * 100.0
            } else {
                0.0
            };
            let htxt = if headroom >= 0.0 {
                format!("{:.0}% headroom", headroom)
            } else {
                format!("{:.0}% over budget", -headroom)
            };
            ui.label(
                egui::RichText::new(htxt)
                    .small()
                    .color(meter_color((frame_ms / (PERF_BUDGET_MS * 2.0)).clamp(0.0, 1.0))),
            );
        });

        ui.add_space(12.0);

        // ── Right: the scrolling frame-time graph (fills the rest) ───────
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("FRAME TIME   ·   last few seconds")
                    .small()
                    .weak(),
            );
            let w = ui.available_width().max(120.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::hover());
            paint_frametime_graph(&ui.painter_at(rect), rect, &state.perf_hist);
        });
    });

    // ── Tier 2: the workload tiles (CPU ms, instances, scale, fill, TLAS) ──
    if let Some(f) = feedback {
        ui.add_space(2.0);
        ui.separator();
        let tw = 96.0;
        ui.horizontal(|ui| {
            // GPU frame time vs the 60 fps budget (Tier 3). "n/a" without
            // timestamp-query support.
            let (gpu_val, gpu_frac) = if f.gpu_timing_available != 0 {
                (format!("{:.1} ms", f.gpu_ms), f.gpu_ms / (PERF_BUDGET_MS * 2.0))
            } else {
                ("n/a".to_string(), 0.0)
            };
            stat_tile(ui, tw, "GPU / frame", gpu_val, gpu_frac);
            ui.add_space(6.0);
            // CPU encode cost vs the 60 fps budget.
            stat_tile(
                ui,
                tw,
                "CPU / frame",
                format!("{:.1} ms", f.cpu_ms),
                f.cpu_ms / (PERF_BUDGET_MS * 2.0),
            );
            ui.add_space(6.0);
            // Instances drawn — log-scaled against a ~200k soft ceiling. 0 on
            // raymarch paths (mandelbulb / KIFS / implicit surfaces), which is honest.
            let inst_frac = if f.instances > 0 {
                (f.instances as f32).max(1.0).log10() / 200_000f32.log10()
            } else {
                0.0
            };
            stat_tile(ui, tw, "Instances", fmt_count(f.instances), inst_frac);
            ui.add_space(6.0);
            // Render scale — 100% = full headroom (green); DRS dropping it = load.
            stat_tile(
                ui,
                tw,
                "Render scale",
                format!("{}%", (f.scale * 100.0).round() as i32),
                1.0 - f.scale.clamp(0.0, 1.0),
            );
            ui.add_space(6.0);
            // Pixel fill — the render megapixels, against an 8.3 MP (4K) ceiling.
            let mp = (f.width as f32 * f.height as f32) / 1.0e6;
            stat_tile(ui, tw, "Fill (px)", format!("{:.1} MP", mp), mp / 8.3);
            ui.add_space(6.0);
            // Hardware-RT TLAS rebuild cost (0 / "off" when RT isn't building).
            let (tlas_val, tlas_frac) = if f.tlas_ms > 0.0 {
                (format!("{:.2} ms", f.tlas_ms), f.tlas_ms / PERF_BUDGET_MS)
            } else {
                ("off".to_string(), 0.0)
            };
            stat_tile(ui, tw, "RT TLAS", tlas_val, tlas_frac);
        });
    }
    ui.add_space(2.0);
}

/// The Audio Reactivity panel: live input meter + spectrum analyzer, the five
/// band meters (the audio sources), and the analysis tuning. Routing of these
/// sources into the visualization is the next step (see Pulse Routing for now).
/// The Environment panel: the whole world layer (terrain, atmosphere & water, and
/// the HDR starfield + sun) as sectioned cards in a floating window. These are
/// global display layers — not per-preset looks — so they live outside the main
/// parameter grid.
/// Capture / production-frame panel (#135 Phase 1). `out_readout` is the live
/// output size from the visual's feedback channel ((0,0) / None = Native).
/// The capture / production-frame stack (#135) — one column of cards on the
/// Settings tab (was the floating 🎬 panel). `w` is the usable row width
/// measured from the owning column, like every other card column.
fn capture_ui(
    ui: &mut egui::Ui,
    w: f32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    out_readout: Option<(u32, u32)>,
) {
    card(ui, "Output", |ui| {
        srow(ui, w, "aspect", &params.aspect_preset, setter);
        srow(ui, w, "long edge (0=display)", &params.out_long_edge, setter);
        help(ui, "long edge 0 = match the display (full native res, no downscale) — \
                 e.g. 16:9 → 4K on a 4K projector. For max sharpness keep Render Scale \
                 at 1.0 (Renderer card).");
        ui.label(egui::RichText::new("— custom (aspect = Custom) —").weak().small());
        srow(ui, w, "width (px)", &params.out_custom_w, setter);
        srow(ui, w, "height (px)", &params.out_custom_h, setter);
        let res = match out_readout {
            Some((wd, ht)) if wd > 0 && ht > 0 => format!("● output {wd}×{ht}"),
            _ => "● output: Native (window)".to_string(),
        };
        ui.label(egui::RichText::new(res).weak().small());
        help(ui, "Renders into a fixed-resolution frame and letterboxes it into the \
                 window, so an OBS capture is pixel-exact. Native = render straight to \
                 the window. A per-display setting (not preset-saved), like HDR/MSAA.");
    });

    card(ui, "Letterbox & Guides", |ui| {
        srow(ui, w, "bar R", &params.letterbox_r, setter);
        srow(ui, w, "bar G", &params.letterbox_g, setter);
        srow(ui, w, "bar B", &params.letterbox_b, setter);
        crow(ui, "frame guide ('G' in the visual)", &params.frame_guide, setter);
        crow(ui, "lock window to output", &params.lock_window, setter);
    });

    card(ui, "Overlay (#135 P2)", |ui| {
        crow(ui, "enable ('T' in the visual)", &params.overlay_enabled, setter);
        srow(ui, w, "opacity", &params.overlay_opacity, setter);
        srow(ui, w, "scale", &params.overlay_scale, setter);
        ui.label(egui::RichText::new("— zones —").weak().small());
        crow(ui, "title", &params.overlay_title, setter);
        crow(ui, "description", &params.overlay_desc, setter);
        crow(ui, "formula", &params.overlay_formula, setter);
        crow(ui, "readouts", &params.overlay_readouts, setter);
        crow(ui, "handle", &params.overlay_handle, setter);
        ui.label(egui::RichText::new("— panel —").weak().small());
        srow(ui, w, "panel R", &params.overlay_panel_r, setter);
        srow(ui, w, "panel G", &params.overlay_panel_g, setter);
        srow(ui, w, "panel B", &params.overlay_panel_b, setter);
        srow(ui, w, "panel opacity", &params.overlay_panel_opacity, setter);
        ui.label(egui::RichText::new("— text colour —").weak().small());
        srow(ui, w, "text R", &params.overlay_text_r, setter);
        srow(ui, w, "text G", &params.overlay_text_g, setter);
        srow(ui, w, "text B", &params.overlay_text_b, setter);
        help(ui, "Title / description / formula come from the generator's metadata; \
                 the readout panel shows its live values. Handle + title override are \
                 set below. Per-display (not preset-saved).");
    });

    card(ui, "Axes & Volume (#135 P5)", |ui| {
        crow(ui, "axes ('X' in the visual)", &params.axes_on, setter);
        srow(ui, w, "axis length", &params.axes_len, setter);
        srow(ui, w, "axis thickness", &params.axes_thick, setter);
        srow(ui, w, "axis opacity", &params.axes_opacity, setter);
        crow(ui, "tick marks", &params.axes_ticks, setter);
        crow(ui, "X/Y/Z labels", &params.axes_labels, setter);
        ui.label(egui::RichText::new("— wireframe box —").weak().small());
        crow(ui, "box", &params.box_on, setter);
        srow(ui, w, "box extent", &params.box_extent, setter);
        srow(ui, w, "subdivisions", &params.box_subdiv, setter);
        srow(ui, w, "box R", &params.box_r, setter);
        srow(ui, w, "box G", &params.box_g, setter);
        srow(ui, w, "box B", &params.box_b, setter);
        srow(ui, w, "box opacity", &params.box_opacity, setter);
        help(ui, "World-space reference axes (X red / Y green / Z blue) + an optional \
                 wireframe box, drawn in the scene with depth. 'X' hides/shows all of it.");
    });

    card(ui, "Field Chamber (#346)", |ui| {
        crow(ui, "panels (analyzer walls)", &params.panels_on, setter);
        param_combo(ui, w, "style", &params.panel_style, setter);
        crow(ui, "rear −Z = oscilloscope", &params.panel_rear, setter);
        crow(ui, "right +X = spectrum", &params.panel_right, setter);
        crow(ui, "camera-relative walls", &params.panel_wall_rel, setter);
        srow(ui, w, "opacity", &params.panel_opacity, setter);
        srow(ui, w, "wall fill", &params.panel_fill, setter);
        srow(ui, w, "line thickness", &params.panel_thickness, setter);
        srow(ui, w, "emissive", &params.panel_emissive, setter);
        ui.label(egui::RichText::new("— impostor material (style = Impostor) —").weak().small());
        param_combo(ui, w, "material", &params.panel_material, setter);
        srow(ui, w, "metallic", &params.panel_metallic, setter);
        srow(ui, w, "roughness", &params.panel_roughness, setter);
        help(ui, "Hangs the calibrated oscilloscope (rear wall = time) + spectrum \
                 (right wall = frequency) on the box's back walls, so the Duo-Field sits \
                 inside a time × frequency frame. The scope + spectrum use the SAME settings \
                 as the Audio tab's analyzer (scope time/amp/trigger/channel + the RTA \
                 resolution/weighting) — no separate dials here. Drawn only on back-facing \
                 walls (never occludes the field). Flat = 2-D composite; Impostor = rounded \
                 chrome/glass lines. Off by default. Captured in presets (Look).");
    });
}

/// The world layer (was the floating 🌍 panel), laid out across the Environment
/// tab's three columns: land (Terrain, Atmosphere & Water) / sky (Sun & Day
/// Cycle, Atmosphere, Clouds) / sea + night (Ocean, Starfield). `w` is the
/// usable row width measured from the first column (all three are equal).
fn environment_ui(c: &mut [egui::Ui], w: f32, params: &OrganicMathParams, setter: &ParamSetter) {
    card(&mut c[0], "Terrain", |ui| {
        crow(ui, "enable (fly over mountains)", &params.terrain_enabled, setter);
        param_combo(ui, w, "noise", &params.terrain_noise, setter);
        param_combo(ui, w, "palette", &params.terrain_palette, setter);
        crow(ui, "ridged (alpine)", &params.terrain_ridged, setter);
        srow(ui, w, "seed", &params.terrain_seed, setter);
        srow(ui, w, "height", &params.terrain_height, setter);
        srow(ui, w, "snow line", &params.terrain_snow, setter);
        srow(ui, w, "fly speed", &params.terrain_scroll, setter);
        srow(ui, w, "ride height", &params.terrain_ride, setter);
        ui.label(egui::RichText::new("— performance —").weak().small());
        srow(ui, w, "march steps", &params.terrain_steps, setter);
        srow(ui, w, "march octaves", &params.terrain_octaves, setter);
        param_combo(ui, w, "resolution", &params.terrain_res, setter);
        help(ui, "An infinite raymarched landscape behind any generator — the generator \
                 plays in the sky while a synthetic camera flies over the mountains. \
                 Replaces the skybox while on; the IBL still lights the geometry. \
                 Perf: drop march steps/octaves or set resolution to Half/Quarter/Eighth \
                 on a projector.");
    });

    card(&mut c[1], "Sun & Day Cycle", |ui| {
        srow(ui, w, "sun elevation", &params.terrain_sun_elev, setter);
        srow(ui, w, "sun azimuth", &params.terrain_sun_azim, setter);
        srow(ui, w, "sun intensity", &params.terrain_sun_int, setter);
        srow(ui, w, "day speed", &params.terrain_day_speed, setter);
        crow(ui, "sun lights the generator", &params.terrain_sun_scene, setter);
        help(ui, "The day cycle drives one sun shared by the terrain, the generator key \
                 light, the HDR sun disc, and the starfield's night fade. Day speed > 0 \
                 sweeps the sun through elevation (rise/set); 0 holds it at the angle above.");
    });

    card(&mut c[1], "Atmosphere (physical sky)", |ui| {
        crow(ui, "enable (derived scattering sky)", &params.atmos_enabled, setter);
        srow(ui, w, "turbidity (haze)", &params.atmos_turbidity, setter);
        srow(ui, w, "mie g (sun halo)", &params.atmos_mie_g, setter);
        srow(ui, w, "sun intensity", &params.atmos_sun_int, setter);
        srow(ui, w, "rayleigh (blue)", &params.atmos_rayleigh, setter);
        srow(ui, w, "ground albedo", &params.atmos_ground_albedo, setter);
        srow(ui, w, "exposure", &params.atmos_exposure, setter);
        srow(ui, w, "aerial perspective", &params.atmos_aerial, setter);
        help(ui, "Physically based single-scattering sky (Rayleigh + Mie) — derived, not \
                 tuned, so it's correct at every sun angle: blue zenith, the full sunset \
                 gradient, the Mie halo hugging the sun, the reddened low sun, blue hour. \
                 Baked into the IBL, so the geometry is lit by the real sky at the real sun \
                 angle (amber at sunset, cool at noon). Drives the terrain sky + aerial \
                 perspective too. ON by default (the default environment); a loaded .hdr \
                 overrides it. Sun follows the day cycle. GPU cost: a re-bake on each \
                 ~degree of sun motion.");
    });

    card(&mut c[0], "Atmosphere & Water", |ui| {
        srow(ui, w, "fog", &params.terrain_fog, setter);
        srow(ui, w, "haze", &params.terrain_haze, setter);
        srow(ui, w, "brightness", &params.terrain_brightness, setter);
        srow(ui, w, "emissive (HDR glow)", &params.terrain_emissive, setter);
        srow(ui, w, "scattering", &params.terrain_scatter, setter);
        srow(ui, w, "god rays", &params.terrain_godray, setter);
        ui.label(egui::RichText::new("— water —").weak().small());
        crow(ui, "sea level (reflective)", &params.terrain_water, setter);
        srow(ui, w, "water level", &params.terrain_water_level, setter);
        srow(ui, w, "water hue", &params.terrain_water_hue, setter);
        srow(ui, w, "water ripple", &params.terrain_water_ripple, setter);
        help(ui, "Emissive makes the land glow (lava/biolume per palette, HDR); scattering \
                 + god rays add aerial atmosphere; water floods the valleys with a \
                 reflective, rippling sea. (Terrain must be on.)");
    });

    card(&mut c[1], "Volumetric Clouds", |ui| {
        crow(ui, "enable (raymarched clouds)", &params.clouds_enabled, setter);
        srow(ui, w, "coverage", &params.clouds_coverage, setter);
        srow(ui, w, "density", &params.clouds_density, setter);
        srow(ui, w, "detail (erosion)", &params.clouds_detail, setter);
        srow(ui, w, "base altitude", &params.clouds_base, setter);
        srow(ui, w, "thickness", &params.clouds_thickness, setter);
        srow(ui, w, "drift speed", &params.clouds_drift, setter);
        ui.label(egui::RichText::new("— lighting —").weak().small());
        srow(ui, w, "forward scatter (silver)", &params.clouds_hg, setter);
        srow(ui, w, "absorption", &params.clouds_absorption, setter);
        srow(ui, w, "ambient fill", &params.clouds_ambient, setter);
        srow(ui, w, "shadow on terrain", &params.clouds_shadow, setter);
        ui.label(egui::RichText::new("— performance —").weak().small());
        srow(ui, w, "march steps", &params.clouds_steps, setter);
        help(ui, "A raymarched volumetric cloud layer (coverage/erosion density, a light \
                 march for self-shadowing → silver linings + sun-behind glow, Henyey–\
                 Greenstein forward scatter) replacing the flat cloud sheet. Lit \
                 consistently with the day cycle / atmosphere — sunset clouds glow amber \
                 — and casts soft shadows on the land. Lives in the terrain sky, so \
                 Terrain must be on. PERF: it's the heaviest world effect — drop march \
                 steps (and the Terrain resolution) on a projector.");
    });

    card(&mut c[2], "FFT Ocean", |ui| {
        crow(ui, "enable (Tessendorf waves)", &params.ocean_enabled, setter);
        help(ui, "Tip: turn Terrain OFF for an infinite ocean-only world.");
        srow(ui, w, "level (world y)", &params.ocean_level, setter);
        srow(ui, w, "wind speed", &params.ocean_wind_speed, setter);
        srow(ui, w, "wind direction", &params.ocean_wind_dir, setter);
        srow(ui, w, "amplitude", &params.ocean_amplitude, setter);
        srow(ui, w, "choppiness", &params.ocean_choppiness, setter);
        srow(ui, w, "tile size (scale)", &params.ocean_tile_size, setter);
        ui.label(egui::RichText::new("— look —").weak().small());
        srow(ui, w, "hue", &params.ocean_hue, setter);
        srow(ui, w, "depth absorption", &params.ocean_depth, setter);
        srow(ui, w, "foam", &params.ocean_foam, setter);
        srow(ui, w, "sun glitter", &params.ocean_glitter, setter);
        help(ui, "A statistical wind-wave ocean: a Phillips spectrum → inverse FFT → a \
                 tiling wave field (correct energy across scales), replacing the pooled \
                 water. Fresnel-reflects the sky (incl. the #100 atmosphere), with \
                 depth absorption, sun glitter, and foam on the crests. Wind speed sets \
                 the sea state; choppiness sharpens crests + drives foam; tile size is \
                 the wave scale. Lives in the terrain pass — keep Terrain on for an \
                 island sea, or turn it OFF for open ocean.");
    });

    card(&mut c[2], "Starfield", |ui| {
        crow(ui, "enable (9110 real stars)", &params.stars_enabled, setter);
        srow(ui, w, "brightness", &params.stars_brightness, setter);
        srow(ui, w, "magnitude limit", &params.stars_mag_limit, setter);
        srow(ui, w, "saturation", &params.stars_saturation, setter);
        srow(ui, w, "star size", &params.stars_size, setter);
        srow(ui, w, "twinkle", &params.stars_twinkle, setter);
        srow(ui, w, "twinkle speed", &params.stars_twinkle_speed, setter);
        srow(ui, w, "latitude", &params.stars_latitude, setter);
        srow(ui, w, "sky rotation", &params.stars_sky_speed, setter);
        ui.label(egui::RichText::new("— sun disc —").weak().small());
        crow(ui, "sun disc (HDR)", &params.stars_sun, setter);
        srow(ui, w, "sun brightness", &params.stars_sun_bright, setter);
        srow(ui, w, "sun size", &params.stars_sun_size, setter);
        srow(ui, w, "sun warmth", &params.stars_sun_warmth, setter);
        help(ui, "The real Yale Bright Star Catalog as additive HDR points — bright stars \
                 bloom and use the EDR headroom; colours follow spectral type. Stars fade \
                 in as the sun sets (Sun & Day Cycle), so push the sun below the horizon \
                 (or run the day cycle into night) to see them. Latitude sets the pole \
                 height; sky rotation wheels the sky. Magnitude limit thins density. The \
                 sun disc rides the same day-cycle sun direction.");
    });
}

/// The Audio instrument tab (#333): a three-column performance layout —
/// **Levels & Loudness** | **Spectrum** | **Oscilloscope** — laid out so the
/// glanceable meters, the frequency view, and the time-domain scope each own a
/// column. Was the floating 🎵 Audio window.
/// The #482 Tier 1 Live-Telemetry dashboard: the audio-panel-style widgets that
/// light up per token, all driven from the latest `MindFrame` already folded into
/// `state.mind_viz`. A 3-column grid mirroring the Audio tab's layout. Idle (no
/// writer / not streaming) → every widget renders flat, exactly like Audio silence.
/// Pump the Mind telemetry for this frame: open the ring reader if it isn't open, hand
/// the newest frame to the dashboard's smoothers, and keep repainting while a model
/// streams.
///
/// Split out of the dashboard body (#532 Tier 1) because the two editions now call it
/// from different places: Organon Mind ticks it once per frame for the always-visible
/// bottom dock, full Organon ticks it inside the Mind tab where the dashboard is still
/// inline. mmap reads are non-destructive, so the editor's own reader never disturbs
/// the visual's.
fn mind_observe(ctx: &egui::Context, state: &mut preset::PresetUi) {
    if state
        .mind_reader
        .as_ref()
        .map(|r| !r.is_open())
        .unwrap_or(true)
    {
        state.mind_reader = Some(crate::mind_ring::MindRingReader::open());
    }
    let frame = state.mind_reader.as_ref().and_then(|r| r.latest());
    let now = ctx.input(|i| i.time);
    let dt = ctx.input(|i| i.stable_dt);
    state.mind_viz.observe(frame.as_ref(), now, dt);
    if state.mind_viz.active {
        ctx.request_repaint();
    }
}

fn mind_dashboard_ui(ui: &mut egui::Ui, state: &mut preset::PresetUi) {
    use crate::mind_viz::{provenance_row, Provenance};
    ui.label(egui::RichText::new("Live Telemetry").color(theme::BONE()).strong());
    ui.label(
        egui::RichText::new(
            "The model's oscilloscope. Widgets light up per token while it infers — \
             from the activations we already stream, cross-model (any .gguf). Each \
             widget's tag (= measured · ~ derived · ? proxy) explains itself on hover.",
        )
        .weak()
        .small(),
    );

    let viz = &state.mind_viz;
    // The dashboard is ALWAYS compact. It lives in the bottom dock now, and the dock
    // is budgeted for exactly this layout (`mind_shell::DASHBOARD_H`) — the taller
    // variant did not fit and was silently clipped at the panel edge, which is what
    // the old `compact` checkbox was really working around. One layout, sized to its
    // container, beats a toggle between "cut off" and "less cut off".
    let (h_depth, h_heat, h_hb, h_topk) = (86.0, 86.0, 58.0, 108.0);
    let alloc = |ui: &mut egui::Ui, h: f32| -> egui::Rect {
        let w = ui.available_width().max(120.0);
        ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover()).0
    };

    fixed_columns(ui, |c| {
        // ── Column 0: Depth profile + MLP rail + Context gauge ───────────
        card(&mut c[0], "Depth profile", |ui| {
            // #522 Tier 1 — this is the glyph the whole tap exists to flip. `=` once the
            // runtime tapped real `l_out-{N}` tensors, `?` while it is shaping the
            // entropy proxy. The widget itself is unchanged; only its claim is.
            provenance_row(
                ui,
                viz.depth_provenance(),
                "activation norm per layer · watch the bulge climb",
            );
            let rect = alloc(ui, h_depth);
            viz.paint_depth(&ui.painter_at(rect), rect);
        });
        // Context / KV fuel gauge (#482 Tier 3) — measured counts from the runtime.
        card(&mut c[0], "Context (KV)", |ui| {
            provenance_row(ui, Provenance::Measured, "how full the context window is");
            let rect = alloc(ui, 20.0);
            viz.paint_context(&ui.painter_at(rect), rect);
        });

        // ── Column 1: Head × layer heat strip + Heartbeat ────────────────
        card(&mut c[1], "Head × layer", |ui| {
            // Stays `?` in #522 Tier 1 on purpose: per-head attention needs the
            // attention matrix to materialize (flash-attention off), which is Tier 2's
            // trade to make. Promoting this glyph with the others would over-claim.
            provenance_row(
                ui,
                Provenance::Proxy,
                "attention summary · layer × head · brightness = activity",
            );
            let rect = alloc(ui, h_heat);
            viz.paint_heat(&ui.painter_at(rect), rect);
        });
        card(&mut c[1], "Heartbeat", |ui| {
            // Measured once the stream carries entropy/confidence; proxy (effort) before.
            let prov = if viz.has_stats() {
                Provenance::Measured
            } else {
                Provenance::Proxy
            };
            provenance_row(
                ui,
                prov,
                "next-token entropy + confidence, one sample per token",
            );
            let rect = alloc(ui, h_hb);
            viz.paint_heartbeat(&ui.painter_at(rect), rect);
        });

        // ── Column 2: Next-token top-k + Tempo of thought ────────────────
        card(&mut c[2], "Next token (top-k)", |ui| {
            provenance_row(
                ui,
                Provenance::Measured,
                "the actual softmax — watch it weigh the / a / this…",
            );
            let rect = alloc(ui, h_topk);
            viz.paint_topk(&ui.painter_at(rect), rect);
            if viz.topk_count == 0 {
                ui.label(
                    egui::RichText::new("(no stream — run a model or the mind-writer demo)")
                        .small()
                        .weak(),
                );
            }
        });
        card(&mut c[2], "Tempo of thought", |ui| {
            provenance_row(ui, Provenance::Derived, "tokens/sec + counters from frame timing");
            ui.horizontal(|ui| {
                let (txt, col) = if viz.active {
                    ("● thinking", egui::Color32::from_rgb(80, 200, 110))
                } else if viz.token_index > 0 || !viz.effort.is_empty() {
                    ("● idle", egui::Color32::from_rgb(150, 130, 90))
                } else {
                    ("● no stream", egui::Color32::from_rgb(110, 116, 130))
                };
                ui.label(egui::RichText::new(txt).color(col).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{:.1} tok/s", viz.tps.max(0.0)))
                            .monospace(),
                    );
                });
            });
        });
    });
}

fn audio_instrument_ui(
    ui: &mut egui::Ui,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    state: &mut preset::PresetUi,
    viz: &audio::AudioViz,
    scope: &audio::ScopeRing,
) {
    let frame = viz.snapshot();
    let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
    let enabled = params.audio_react.value();
    fixed_columns(ui, |c| {
        let w0 = (c[0].available_width() - COL_PAD).max(150.0);
        let w1 = (c[1].available_width() - COL_PAD).max(150.0);
        let w2 = (c[2].available_width() - COL_PAD).max(150.0);

        // ── Column 0: Levels & Loudness ──────────────────────────────────
        card(&mut c[0], "Input", |ui| {
            audio_status_row(ui, params, setter, &frame, enabled);
            audio_input_meter(ui, state, &frame, dt);
        });
        card(&mut c[0], "Loudness (BS.1770)", |ui| {
            audio_cal_meters(ui, &frame);
        });
        card(&mut c[0], "Analysis Tuning", |ui| {
            srow(ui, w0, "gain", &params.audio_gain, setter);
            srow(ui, w0, "attack", &params.audio_attack, setter);
            srow(ui, w0, "release", &params.audio_release, setter);
            help(ui, "Gain scales sensitivity (turn up for quiet input). Attack/release \
                     shape the envelope follower. Clock + pulse source live in \
                     Settings › Sync / Tempo.");
        });
        card(&mut c[0], "Duo-Field Instrument (Tier 3)", |ui| {
            param_combo(ui, w0, "drive", &params.analytical_mode, setter);
            srow(ui, w0, "target LUFS", &params.an_target_lufs, setter);
            srow(ui, w0, "floor LUFS", &params.an_floor_lufs, setter);
            srow(ui, w0, "TP ceiling", &params.an_tp_ceiling, setter);
            srow(ui, w0, "corr alarm", &params.an_corr_alarm, setter);
            crow(ui, "instrument HUD (visual)", &params.an_reference_hud, setter);
            help(ui, "Calibrated drive: the Acoustic/Maxwell field is driven by the \
                     MEASURED loudness (LUFS, gain-independent) via a reproducible dB \
                     law instead of the expressive gain·RMS — the same track makes the \
                     same field on any machine. Needs 'audio drives the source' on. \
                     Target/floor set the drive curve + the HUD's over/under-target \
                     horizon; TP ceiling + corr alarm flash the instrument HUD. \
                     Expressive (default) → today's look, byte-identical.");
        });

        // ── Column 1: Spectrum ───────────────────────────────────────────
        card(&mut c[1], "Spectrum (FFT)", |ui| {
            audio_spectrum(ui, state, &frame, dt);
        });
        card(&mut c[1], "Calibrated RTA", |ui| {
            audio_rta(ui, w1, params, setter, state, &frame);
        });

        // ── Column 2: Oscilloscope ───────────────────────────────────────
        card(&mut c[2], "Oscilloscope", |ui| {
            audio_scope(ui, w2, params, setter, state, scope);
        });
        card(&mut c[2], "Pulse Routing", |ui| {
            param_combo(ui, w2, "A target", &params.mod_a_target, setter);
            srow(ui, w2, "A depth", &params.mod_a_depth, setter);
            param_combo(ui, w2, "B target", &params.mod_b_target, setter);
            srow(ui, w2, "B depth", &params.mod_b_depth, setter);
            help(ui, "Two slots route the pulse envelope (beat or audio bass) to any \
                     param, with bipolar depth. Active only while Pulse is on.");
        });
    });
}

/// Audio-Reactive enable + a live status light (off / clip / live / silent).
fn audio_status_row(
    ui: &mut egui::Ui,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    frame: &audio::VizFrame,
    enabled: bool,
) {
    ui.horizontal(|ui| {
        crow(ui, "Audio Reactive (analyze)", &params.audio_react, setter);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (txt, col) = if !enabled {
                ("● off", egui::Color32::from_rgb(110, 116, 130))
            } else if frame.peak >= 1.0 {
                ("● CLIP", egui::Color32::from_rgb(235, 70, 55))
            } else if frame.level > 1.0e-3 {
                ("● live", egui::Color32::from_rgb(80, 200, 110))
            } else {
                ("● silent", egui::Color32::from_rgb(150, 130, 90))
            };
            ui.label(egui::RichText::new(txt).color(col).strong());
        });
    });
}

/// Vertical input VU (peak-held) + a numeric level/peak readout.
fn audio_input_meter(ui: &mut egui::Ui, state: &mut preset::PresetUi, frame: &audio::VizFrame, dt: f32) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("IN").small().weak());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 128.0), egui::Sense::hover());
            let lt = meter_norm(frame.level);
            state.level_peak = if lt >= state.level_peak {
                lt
            } else {
                (state.level_peak - 0.6 * dt).max(lt).max(0.0)
            };
            paint_vmeter(&ui.painter_at(rect), rect, lt, state.level_peak);
        });
        ui.add_space(6.0);
        ui.vertical(|ui| {
            let db = |lin: f32| if lin > 1.0e-6 { format!("{:.1} dBFS", 20.0 * lin.log10()) } else { "−∞".into() };
            ui.add_space(6.0);
            ui.label(egui::RichText::new(format!("level {}", db(frame.level))).monospace().small());
            ui.label(egui::RichText::new(format!("peak  {}", db(frame.peak))).monospace().small());
        });
    });
}

/// The BS.1770 calibrated-meter grid (LUFS M/S/I, LRA, dBTP, correlation, L/R/M/S).
fn audio_cal_meters(ui: &mut egui::Ui, frame: &audio::VizFrame) {
    let dbfmt = |v: f32| -> String {
        if v <= -119.0 || !v.is_finite() { "  −∞".to_string() } else { format!("{v:5.1}") }
    };
    let m = &frame.meters;
    egui::Grid::new("cal_meters").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
        let cell = |ui: &mut egui::Ui, k: &str, v: String| {
            ui.label(egui::RichText::new(format!("{k} {v}")).monospace());
        };
        cell(ui, "LUFS-M", dbfmt(m[0]));
        cell(ui, "LUFS-S", dbfmt(m[1]));
        cell(ui, "LUFS-I", dbfmt(m[2]));
        cell(ui, "LRA", format!("{:4.1}", m[3].max(0.0)));
        ui.end_row();
        cell(ui, "dBTP ", dbfmt(m[4]));
        cell(ui, "corr ", format!("{:+.2}", m[5]));
        cell(ui, "M dBFS", dbfmt(m[8]));
        cell(ui, "S dBFS", dbfmt(m[9]));
        ui.end_row();
        cell(ui, "L dBFS", dbfmt(m[6]));
        cell(ui, "R dBFS", dbfmt(m[7]));
        let crest = if m[4] > -119.0 && m[6] > -119.0 { m[4] - m[6].max(m[7]) } else { f32::NAN };
        cell(ui, "crest", if crest.is_finite() { format!("{crest:4.1}") } else { "  —".into() });
        if m[4] > -1.0 {
            ui.label(egui::RichText::new("⚠ TP").color(egui::Color32::from_rgb(235, 70, 55)).strong());
        }
        ui.end_row();
    });
}

/// The expressive FFT spectrum display + the five band envelopes.
fn audio_spectrum(ui: &mut egui::Ui, state: &mut preset::PresetUi, frame: &audio::VizFrame, dt: f32) {
    ui.label(egui::RichText::new("30 Hz → 16 kHz (log)").small().weak());
    let w = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 140.0), egui::Sense::hover());
    paint_spectrum(&ui.painter_at(rect), rect, &frame.spectrum, &mut state.spectrum_peaks, dt);
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Bands").color(theme::BONE()).strong());
    for b in 0..audio::NUM_BANDS {
        band_bar(ui, audio::BAND_LABELS[b], frame.bands[b]);
    }
}

/// The calibrated RTA: controls + a dB-axis bar graph + a spectrogram/waterfall.
fn audio_rta(
    ui: &mut egui::Ui,
    w: f32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    state: &mut preset::PresetUi,
    frame: &audio::VizFrame,
) {
    param_combo(ui, w, "resolution", &params.meter_res, setter);
    param_combo(ui, w, "weighting", &params.meter_weight, setter);
    param_combo(ui, w, "averaging", &params.meter_averaging, setter);
    crow(ui, "show HUD in visual", &params.meter_hud, setter);
    let rta_w = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(rta_w, 120.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, egui::Color32::from_rgb(14, 16, 24));
    let (db_lo, db_hi) = (-72.0f32, 0.0f32);
    let yof = |db: f32| rect.bottom() - (db.clamp(db_lo, db_hi) - db_lo) / (db_hi - db_lo) * rect.height();
    let mut g = db_hi;
    while g >= db_lo {
        let y = yof(g);
        p.hline(rect.x_range(), y, egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 44, 56)));
        p.text(egui::pos2(rect.left() + 2.0, y), egui::Align2::LEFT_TOP,
            format!("{g:.0}"), egui::FontId::monospace(9.0), egui::Color32::from_rgb(120, 126, 140));
        g -= 12.0;
    }
    let nb = frame.rta_n.max(1);
    let bw = rect.width() / nb as f32;
    for i in 0..frame.rta_n {
        let db = frame.rta[i];
        if !db.is_finite() || db <= db_lo {
            continue;
        }
        let x = rect.left() + i as f32 * bw;
        let hue = i as f32 / nb as f32;
        let col = egui::Color32::from_rgb((60.0 + 195.0 * hue) as u8, 180, (255.0 - 150.0 * hue) as u8);
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x, yof(db)), egui::pos2(x + bw * 0.85, rect.bottom())), 0.0, col);
    }
    if frame.rta_c0 > 0.0 && frame.rta_res > 0 {
        for dec in [100.0f32, 1000.0, 10000.0] {
            let idx = (frame.rta_res as f32 * (dec / frame.rta_c0).log2()).round();
            if idx >= 0.0 && (idx as usize) < frame.rta_n {
                let x = rect.left() + idx * bw;
                let lbl = if dec >= 1000.0 { format!("{:.0}k", dec / 1000.0) } else { format!("{dec:.0}") };
                p.text(egui::pos2(x, rect.bottom() - 11.0), egui::Align2::LEFT_BOTTOM, lbl,
                    egui::FontId::monospace(9.0), egui::Color32::from_rgb(150, 156, 170));
            }
        }
    }
    // Spectrogram / waterfall (dB colour, newest top).
    let mut row = [-120.0f32; audio::MAX_RTA_BANDS];
    let nvalid = frame.rta_n.min(audio::MAX_RTA_BANDS);
    row[..nvalid].copy_from_slice(&frame.rta[..nvalid]);
    state.rta_waterfall.insert(0, row);
    const ROWS: usize = 64;
    state.rta_waterfall.truncate(ROWS);
    let heat = |t: f32| -> egui::Color32 {
        let r = (255.0 * (t * 1.6 - 0.35).clamp(0.0, 1.0)) as u8;
        let gc = (255.0 * (1.0 - (t - 0.62).abs() * 2.2).clamp(0.0, 1.0)) as u8;
        let b = (255.0 * (1.0 - t * 1.7).clamp(0.0, 1.0)) as u8;
        egui::Color32::from_rgb(r, gc, b)
    };
    let (wrect, _) = ui.allocate_exact_size(egui::vec2(rta_w, 84.0), egui::Sense::hover());
    let wp = ui.painter_at(wrect);
    wp.rect_filled(wrect, 2.0, egui::Color32::from_rgb(8, 10, 16));
    let rh = wrect.height() / ROWS as f32;
    let cw = wrect.width() / nvalid.max(1) as f32;
    for (r, frow) in state.rta_waterfall.iter().enumerate() {
        let y = wrect.top() + r as f32 * rh;
        for i in 0..nvalid {
            let db = frow[i];
            if !db.is_finite() || db <= -72.0 {
                continue;
            }
            let t = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
            let x = wrect.left() + i as f32 * cw;
            wp.rect_filled(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw.max(1.0), rh.max(1.0))), 0.0, heat(t));
        }
    }
    ui.label(egui::RichText::new("spectrogram (dB colour · newest top)").weak().small());
    help(ui, "Calibrated to digital full scale: a full-scale sine reads 0 dBFS in its \
             band. Resolution = fractional-octave (IEC 61260) or linear FFT; weighting = \
             A/C/Z (IEC 61672); averaging = fast/slow/peak-hold/Leq. The meter reads its \
             own input — post whatever precedes it in the chain.");
}

/// Read the most-recent `want` samples of the selected scope channel (0 L, 1 R,
/// 2 Mid) into `buf`, oldest first.
fn scope_read_channel(scope: &audio::ScopeRing, ch: u32, want: usize, buf: &mut Vec<f32>, scratch: &mut Vec<f32>) {
    if ch == 2 {
        scope.read_recent(0, want, buf);
        scope.read_recent(1, want, scratch);
        for (a, b) in buf.iter_mut().zip(scratch.iter()) {
            *a = 0.5 * (*a + *b);
        }
    } else {
        scope.read_recent(ch as usize, want, buf);
    }
}

/// Find the sweep-start index into `buf` for the current trigger mode. Returns
/// `(anchor, triggered)`; `anchor + visible <= buf.len()` is guaranteed on success.
fn scope_anchor(buf: &[f32], visible: usize, total: usize, state: &preset::PresetUi, sr: f32) -> (usize, bool) {
    let maxi = buf.len().saturating_sub(visible);
    match state.scope_trigger {
        1 | 2 => {
            let lvl = state.scope_trig_level;
            let hold = (state.scope_retrigger as usize).min(maxi);
            let rising = state.scope_trigger == 1;
            let mut i = maxi;
            while i > hold.max(1) {
                let cross = if rising {
                    buf[i - 1] < lvl && buf[i] >= lvl
                } else {
                    buf[i - 1] > lvl && buf[i] <= lvl
                };
                if cross {
                    // Retrigger hold-off: the run before the edge must sit on the
                    // pre-edge side of the threshold (stable trigger on dense material).
                    let ok = (1..=hold).all(|k| {
                        let s = buf[i - 1 - k];
                        if rising { s < lvl } else { s > lvl }
                    });
                    if ok {
                        return (i, true);
                    }
                }
                i -= 1;
            }
            (maxi, false)
        }
        3 => {
            // Internal: align the sweep start to an absolute period boundary.
            let p = ((state.scope_internal_ms / 1000.0 * sr).round() as usize).max(1);
            let base = total.saturating_sub(buf.len()); // absolute index of buf[0]
            let anchor_abs = (total.saturating_sub(visible) / p) * p;
            if anchor_abs >= base && anchor_abs.saturating_sub(base) <= maxi {
                (anchor_abs - base, true)
            } else {
                (maxi, false)
            }
        }
        _ => (maxi, false), // Free: newest window, scrolling
    }
}

/// The s(M)exoscope-style oscilloscope: a scrolling waveform with horizontal (TIME)
/// + vertical (AMP) zoom, Free/Rising/Falling/Internal triggering, retrigger hold-off,
/// sync-redraw, freeze, DC-kill, and channel select. All scope processing is GUI-side
/// off the lock-free `ScopeRing`.
fn audio_scope(
    ui: &mut egui::Ui,
    w: f32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    state: &mut preset::PresetUi,
    scope: &audio::ScopeRing,
) {
    if !state.scope_inited {
        state.scope_px_per_sample = 0.5;
        state.scope_internal_ms = 20.0;
        state.scope_inited = true;
    }
    let sr = scope.sample_rate();
    let scope_w = ui.available_width().max(160.0);
    // The scope's core settings are SAVED PARAMS (`scope_*`) shared with the Field
    // Chamber wall — one analyzer, one set of settings. (The chamber no longer has its
    // own copies.) Below these are editor-only VIEW aids that don't affect the picture.
    srow(ui, w, "time (ms)", &params.panel_scope_time_ms, setter);
    srow(ui, w, "amp", &params.panel_scope_amp, setter);
    srow(ui, w, "trigger (0 free/1 rise/2 fall)", &params.panel_scope_trigger, setter);
    srow(ui, w, "channel (0 L/1 R/2 Mid)", &params.panel_scope_channel, setter);
    ui.horizontal(|ui| {
        let mut sync = state.scope_sync_redraw;
        if ui.checkbox(&mut sync, "sync").changed() { state.scope_sync_redraw = sync; }
        let mut frz = state.scope_freeze;
        if ui.checkbox(&mut frz, "freeze").changed() { state.scope_freeze = frz; }
        let mut dck = state.scope_dckill;
        if ui.checkbox(&mut dck, "DC-kill").changed() { state.scope_dckill = dck; }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("retrig").monospace().small());
        ui.add(egui::Slider::new(&mut state.scope_retrigger, 0.0..=4096.0).logarithmic(true));
    });

    // Mirror the shared params into the acquisition state so the display matches the
    // chamber wall exactly (same channel / trigger / window; trigger at zero-crossing).
    state.scope_channel = params.panel_scope_channel.value().clamp(0, 2) as u32;
    state.scope_trigger = params.panel_scope_trigger.value().clamp(0, 2) as u32;
    state.scope_trig_level = 0.0;
    let gain = params.panel_scope_amp.value().max(1.0e-3);
    let visible = ((params.panel_scope_time_ms.value() * sr / 1000.0).round() as usize)
        .clamp(16, scope.capacity() - 1);
    if !state.scope_freeze {
        let search = if state.scope_trigger == 0 { 0 } else { visible.min(8192) };
        let want = (visible + search + 8).min(scope.capacity());
        let total = scope.written();
        let mut buf = Vec::new();
        let mut scratch = Vec::new();
        scope_read_channel(scope, state.scope_channel, want, &mut buf, &mut scratch);
        if state.scope_dckill && !buf.is_empty() {
            let mean = buf.iter().sum::<f32>() / buf.len() as f32;
            for s in buf.iter_mut() {
                *s -= mean;
            }
        }
        let (anchor, triggered) = scope_anchor(&buf, visible, total, state, sr);
        let refresh = !state.scope_sync_redraw || triggered || state.scope_display.len() != visible;
        if refresh && buf.len() >= visible {
            let a = anchor.min(buf.len() - visible);
            state.scope_display = buf[a..a + visible].to_vec();
        }
    }

    // Draw.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(scope_w, 200.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, egui::Color32::from_rgb(6, 8, 12));
    let midy = rect.center().y;
    let half = rect.height() * 0.5;
    // `gain` (linear) comes from the shared `scope amp` param computed above.
    p.hline(rect.x_range(), midy, egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 42, 54)));
    for s in [-1.0f32, 1.0] {
        p.hline(rect.x_range(), midy - s * half, egui::Stroke::new(1.0, egui::Color32::from_rgb(24, 27, 36)));
    }
    if state.scope_trigger == 1 || state.scope_trigger == 2 {
        let ly = (midy - state.scope_trig_level * gain * half).clamp(rect.top(), rect.bottom());
        p.hline(rect.x_range(), ly, egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 120, 60)));
    }
    let disp = &state.scope_display;
    let mut clipped = false;
    if disp.len() >= 2 {
        let n = disp.len();
        let mut pts = Vec::with_capacity(n);
        for (i, &s) in disp.iter().enumerate() {
            let v = s * gain;
            if v.abs() > 1.0 {
                clipped = true;
            }
            let x = rect.left() + i as f32 / (n - 1) as f32 * rect.width();
            let y = midy - v.clamp(-1.15, 1.15) * half;
            pts.push(egui::pos2(x, y));
        }
        p.add(egui::Shape::line(pts, egui::Stroke::new(1.2, egui::Color32::from_rgb(90, 220, 130))));
    }
    if clipped {
        p.text(egui::pos2(rect.right() - 4.0, rect.top() + 3.0), egui::Align2::RIGHT_TOP, "CLIP",
            egui::FontId::monospace(11.0), egui::Color32::from_rgb(235, 70, 55));
    }
    if state.scope_freeze {
        p.text(egui::pos2(rect.left() + 4.0, rect.top() + 3.0), egui::Align2::LEFT_TOP, "❄ FROZEN",
            egui::FontId::monospace(11.0), egui::Color32::from_rgb(120, 190, 235));
    }
    help(ui, "A real-time oscilloscope. time / amp / trigger / channel are the analyzer's \
             SAVED settings (shared with the Field Chamber wall scope — one set drives \
             both): time = window length (ms); amp = vertical gain; trigger \
             (Free/Rising/Falling) stabilises the sweep at the zero-crossing; channel picks \
             L/R/Mid. sync only redraws on a trigger, freeze holds the waveform, DC-kill \
             recenters — these are editor-only view aids. Always live — independent of \
             Audio Reactive.");
}

/// Fixed width for a row's leading label.
/// Minimum width of each of the three docked card columns (`fixed_columns`).
/// Columns divide the available tab width equally (filling it edge-to-edge) but
/// never go below this, so rows stay usable in a cramped window. Column width
/// depends only on the window size — never on content — so cards can't reflow
/// when a value readout changes (the first step toward drag-to-rearrange cards).
const CARD_COL_MIN_W: f32 = 280.0;
/// Per-column horizontal overhead to subtract from the raw column width before
/// using it as the usable row width: the card frame's left+right inner margin
/// (+ a few px safety). Biased a touch large so rows under-fill rather than ever
/// bleed into the next column.
///
/// #542 Tier 1 cut this from 40 to 24: `theme::card_frame`'s 8 pt side margins
/// replaced `Frame::group`'s 6, and `card` now draws its body **unindented** (the
/// hairline rule under the title already says where the header ends, so the 18 pt
/// collapsing-header indent was width spent on a cue we draw twice). All 16 pt of
/// the difference goes to the label segment of every row.
const COL_PAD: f32 = 24.0;

/// Fixed-width value readout + click-to-type editor. `ParamSlider`'s own value
/// text is sized to its string — a ragged right edge that made every row a
/// slightly different width — so `srow` draws the bar `without_value()` and
/// renders the readout itself at a constant `VALUE_W`. Click to type; Enter
/// commits (parsed by the param itself, gesture-wrapped so the host records
/// it); Esc or clicking away cancels. Edit state is keyed by the param's
/// pointer hash (same trick as the recorded-defaults map), so it survives
/// layout shifts and never leaks onto another control.
fn value_box<P: Param>(ui: &mut egui::Ui, param: &P, setter: &ParamSetter) {
    let id = egui::Id::new("om_value_edit").with(hash_of(&param.as_ptr()));
    let buf: Option<String> = ui.data_mut(|d| d.get_temp::<Option<String>>(id)).flatten();
    match buf {
        Some(mut buf) => {
            let resp = ui.add_sized([VALUE_W, 18.0], egui::TextEdit::singleline(&mut buf));
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(n) = param.string_to_normalized_value(&buf) {
                        setter.begin_set_parameter(param);
                        setter.set_parameter_normalized(param, n);
                        setter.end_set_parameter(param);
                    }
                }
                ui.data_mut(|d| d.insert_temp(id, None::<String>));
            } else {
                if !resp.has_focus() {
                    resp.request_focus();
                }
                ui.data_mut(|d| d.insert_temp(id, Some(buf)));
            }
        }
        None => {
            let current = param.unmodulated_normalized_value();
            let text = param.normalized_value_to_string(current, false);
            let resp = ui
                .add_sized(
                    [VALUE_W, 18.0],
                    egui::Button::new(egui::RichText::new(text).small()),
                )
                .on_hover_text("Click to type a value");
            if resp.clicked() {
                let seed = param.normalized_value_to_string(current, false);
                ui.data_mut(|d| d.insert_temp(id, Some(seed)));
            }
        }
    }
}

/// Scalar control row — a fixed grid of `label | bar | value | gap | [⟲/●]`.
/// Every segment is a constant width derived from `avail` (the usable row width
/// measured from the owning column; `available_width()` is unreliable once
/// nested inside the card frame + header), so every row in a card lands on the
/// same grid lines regardless of its label or value.
///
/// The widths come from [`theme::row_grid`] — a pure function, so the arithmetic
/// deciding whether 1057 rows are legible is unit-tested without a window. #542
/// Tier 1 made the **label** the elastic segment: it opens to fit whole names
/// when the column allows and closes to the old fixed 62 pt when it doesn't,
/// instead of holding a constant and starving the slider to its 24 pt floor.
fn srow<P: Param>(ui: &mut egui::Ui, avail: f32, label: &str, param: &P, setter: &ParamSetter) {
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        let g = theme::row_grid(avail, spacing);
        // Still ellipsized: the grid buys room for the names that actually exist
        // in the param table, not a guarantee for any name — a long one may not
        // widen or wrap the row.
        ui.add_sized(
            [g.label_w, 18.0],
            egui::Label::new(egui::RichText::new(label).color(theme::TITANIUM())).truncate(),
        );
        // The bar renders `without_value()` in an exact-size, clipped child UI
        // (`ParamSlider` sizes parts of itself to content, so `add_sized` alone
        // could not cap it); the readout is our own fixed-width `value_box`.
        // Nothing in the row depends on content → a perfect grid.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(g.bar_w, 18.0), egui::Sense::hover());
        let mut sl = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        sl.set_clip_rect(rect.expand(1.0).intersect(sl.clip_rect()));
        sl.add(
            ParamSlider::for_param(param, setter)
                .without_value()
                .with_width(g.bar_w),
        );
        value_box(ui, param, setter);
        ui.add_space(g.gap);
        match default_btn(ui) {
            Some(DefaultAction::Record) => record_default(param),
            Some(DefaultAction::Reset) => reset_one(param, setter),
            None => {}
        }
    });
}

/// Checkbox control row for a `BoolParam` — a real egui checkbox (not the
/// slider-style toggle `srow` would render), gesture-wrapped so the host records
/// the change like any other parameter edit.
fn crow(ui: &mut egui::Ui, label: &str, param: &BoolParam, setter: &ParamSetter) {
    let mut on = param.value();
    if ui.checkbox(&mut on, label).changed() {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, on);
        setter.end_set_parameter(param);
    }
}

/// Discrete-choice control row: `label | dropdown (fills `avail`) | ⟲`. For
/// enum / discrete params (generator, surface mode, material, tone-map, …) — a
/// real `ComboBox` you pick from, replacing `srow`'s drag-through slider for
/// "select one each time" controls. Generic over any `Param` that reports a
/// step count: it walks the discrete steps, labelling each via the param's own
/// value-to-string, and sets the chosen step through the host setter (so it's
/// automation-recordable). Deliberately carries no decade-nudge / record-default
/// affordances — those belong to numeric sliders, not enumerations.
fn param_combo<P: Param>(ui: &mut egui::Ui, avail: f32, label: &str, param: &P, setter: &ParamSetter) {
    param_combo_sized(ui, avail, label, param, setter, COMBO_W);
}

/// `param_combo` with an explicit dropdown width — for the few hero combos
/// (generator algorithm, palette) that have room to breathe at 2× `COMBO_W`.
/// The wider box only eats into the row's flexible gap: the label and the
/// right-aligned three-button group stay on exactly the same grid lines.
fn param_combo_sized<P: Param>(
    ui: &mut egui::Ui,
    avail: f32,
    label: &str,
    param: &P,
    setter: &ParamSetter,
    combo_w: f32,
) {
    ui.horizontal(|ui| {
        let spacing = ui.spacing().item_spacing.x;
        // Shares `srow`'s label width and trailing button, so a combo row and a
        // slider row in the same card land on identical grid lines (#542 T1).
        let g = theme::combo_grid(avail, spacing, combo_w);
        ui.add_sized(
            [g.label_w, 18.0],
            egui::Label::new(egui::RichText::new(label).color(theme::TITANIUM())).truncate(),
        );
        // Fixed-width dropdown — combos no longer stretch to fill the row.
        let w = combo_w;
        let current = param.unmodulated_normalized_value();
        match param.step_count() {
            // `step_count` for a discrete param is (#variants − 1); normalized
            // value i/n ↔ variant i. Round the live value to its variant index.
            Some(n) if n >= 1 => {
                let cur_idx = (current * n as f32).round() as usize;
                let apply = |idx: usize| {
                    let norm = idx as f32 / n as f32;
                    setter.begin_set_parameter(param);
                    setter.set_parameter_normalized(param, norm);
                    setter.end_set_parameter(param);
                };
                // The combo renders inside an exact-size, clipped child UI:
                // `.truncate()` ellipsizes at the ui's available width — the
                // whole row, not `.width(w)` — so a long selected label
                // (generator, palette) still widened the button. The child
                // caps available width at exactly `w`, so truncation happens
                // there and the button can never exceed it.
                let (crect, _) = ui.allocate_exact_size(egui::vec2(w, 18.0), egui::Sense::hover());
                let mut cui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(crect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                // ⚠️ **Clip HORIZONTALLY ONLY. A vertical clip here makes the dropdown
                // unopenable, and it does it silently.**
                //
                // This used to be `crect.expand(1.0)`, i.e. a clip box the same 18 pt
                // height as `crect`. egui does not treat the clip rect as paint-only —
                // `Ui::interact` builds the widget's hit area as
                // `self.clip_rect().intersect(rect)` (egui 0.33 `ui.rs:1140`), so the clip
                // box is also the *clickable* box. And `ComboBox` deliberately makes its
                // button BIGGER than its contents:
                //
                //     let button_rect = ui.min_rect().expand2(ui.spacing().button_padding);
                //     let response = ui.interact(button_rect, button_id, Sense::click());
                //
                // so the padded top and bottom of that button fell outside an 18 pt clip
                // and stopped responding, leaving a sliver — or nothing, once a host-driven
                // `pixels_per_point` rounded the rects differently than the Mac did. The
                // symptom is a dropdown that simply will not open, with no error and no
                // visual hint, while sliders and toggles in the same card work fine
                // (nothing else routes through this clipped child ui).
                //
                // The horizontal clip is the half that was actually wanted: the comment
                // above is about a long selected label *widening* the button, and width is
                // already capped by `max_rect(crect)` + `.truncate()`, with this as the
                // backstop that keeps paint out of the neighbouring column. Vertical
                // clipping served no stated purpose. So keep the column bounds, inherit the
                // parent's vertical clip, and pad by `button_padding` so the button's own
                // expansion stays inside its own column rather than being trimmed at the
                // edges.
                let pad_x = cui.spacing().button_padding.x + 1.0;
                let parent_clip = cui.clip_rect();
                cui.set_clip_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(crect.min.x - pad_x, parent_clip.min.y),
                        egui::pos2(crect.max.x + pad_x, parent_clip.max.y),
                    )
                    .intersect(parent_clip),
                );
                let combo = egui::ComboBox::from_id_salt(label)
                    .width(w)
                    // Ellipsize a long selected label instead of widening the
                    // button past the fixed width (the popup shows full names).
                    .truncate()
                    .selected_text(param.normalized_value_to_string(current, false))
                    .show_ui(&mut cui, |ui| {
                        // The popup is open here. Keyboard-cycle: ↑/↓ move the
                        // selection and **live-apply** it (so the look scrubs as
                        // you move), keeping the popup open; Enter commits the
                        // current selection (caller closes the popup). Consume the
                        // keys so egui's built-in focus nav doesn't also move.
                        let down =
                            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                        let up =
                            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                        let enter =
                            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        let mut sel = cur_idx;
                        if down && sel < n {
                            sel += 1;
                        }
                        if up && sel > 0 {
                            sel -= 1;
                        }
                        let moved = sel != cur_idx;
                        let mut chosen = moved.then_some(sel);
                        for i in 0..=n {
                            let norm = i as f32 / n as f32;
                            let text = param.normalized_value_to_string(norm, false);
                            let resp = ui.selectable_label(i == sel, text);
                            if resp.clicked() {
                                chosen = Some(i);
                            }
                            // Keep the keyboard-selected row in view.
                            if i == sel && moved {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        if let Some(idx) = chosen {
                            apply(idx);
                        }
                        enter
                    });
                // Enter keeps the (already-applied) selection and closes the popup.
                if combo.inner == Some(true) {
                    // egui 0.33 (#541): `Memory::close_popup` took no argument and
                    // closed whatever was open; it now needs the popup's own `Id`,
                    // and the modern entry point is `Popup::close_id` (the `Memory`
                    // one is deprecated internally). `ComboBox::widget_to_popup_id`
                    // is private, so derive the id here: the combo's own response
                    // carries the button id, and the popup hangs off it via
                    // `.with("popup")`.
                    //
                    // Take the id from the response rather than rebuilding it from
                    // the salt: the button id is `make_persistent_id(salt)` on the
                    // ui the combo was *shown* on — `cui`, the clipped child, not
                    // `ui` — and a child `Ui` gets its own distinct id. Rebuilding
                    // it from `ui` silently addresses a popup that never existed,
                    // and would break again if the salt drifted from `from_id_salt`.
                    egui::Popup::close_id(ui.ctx(), combo.response.id.with("popup"));
                }
            }
            // Single-variant / non-discrete — nothing to choose; just show it.
            _ => {
                ui.add_sized(
                    [w, 18.0],
                    egui::Label::new(param.normalized_value_to_string(current, false)),
                );
            }
        }
        // Push the ⟲ to the row's right edge so it lines up with `srow`'s, even
        // though the combo itself is a fixed, narrower width. Enums keep a plain
        // ⟲ (no recorded default — #131).
        ui.add_space(g.gap);
        if reset_btn(ui) {
            reset_one(param, setter);
        }
    });
}

/// #554 Tier 1 — draw the **embedded viewport**: the visual's scene, mirrored into a pane at
/// the top of the editor window.
///
/// The frame arrives as CPU pixels over `frame_ring` rather than as a shared GPU texture,
/// because no *published* `egui-wgpu` pairs with the renderer's wgpu 30 (see `frame_ring`'s
/// module docs). egui does not care what produced the pixels.
///
/// **This is the plugin's path, and #554 Tier 4 is the reason it is now only that.** In-process
/// rendering on the renderer's own device (`ui_layer.rs`, via the vendored wgpu-30 port of
/// `egui-wgpu`) is both accelerated and HDR-capable, neither of which a CPU mirror can be. It
/// needs the process to own its window, which Organon Mind does and a plugin editor inside
/// Ableton does not — so this pane stays exactly as it is for the plugin.
///
/// Three states, and all three have to look deliberate — this pane is the first thing on screen,
/// so "nothing yet" must not read as "broken":
/// - **a frame**: drawn letterboxed, aspect preserved.
/// - **ring open, nothing published**: the visual is up but has not sent a frame yet.
/// - **no ring**: the visual is not running, which is the *normal* case, so it says so and says
///   what to do about it.
///
/// **#593 Tier 4 — full Organon only.** In Organon Mind the surface egui paints on already has
/// the world in it, so a photograph of that world drawn on top of it is not a viewport; it is a
/// lid. Gated rather than deleted because the plugin has no other path (§2.5).
#[cfg(not(feature = "mind-edition"))]
fn viewport_pane(ui: &mut egui::Ui, state: &mut preset::PresetUi) {
    use crate::frame_ring::{FrameRingReader, MIRROR_H, MIRROR_W};

    // Lazily open, and retry a closed ring at ~1 Hz (see `frame_retry`).
    if state.frame_reader.as_ref().map_or(true, |r| !r.is_open()) {
        state.frame_retry = state.frame_retry.wrapping_add(1);
        if state.frame_reader.is_none() || state.frame_retry >= 60 {
            state.frame_retry = 0;
            state.frame_reader = Some(FrameRingReader::open());
        }
    }

    // Upload only when the frame is actually new — `take_latest` returns `None` on a repeat, so
    // a 60 Hz repaint against a ~15 Hz writer re-uploads nothing three times out of four.
    let mut buf = std::mem::take(&mut state.frame_buf);
    let fresh = state.frame_reader.as_mut().and_then(|r| r.take_latest(&mut buf));
    if let Some((w, h)) = fresh {
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &buf);
        match state.frame_tex.as_mut() {
            // Reuse the handle: `set` replaces the texture in place, so the atlas does not grow
            // a new entry every frame.
            Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
            None => {
                state.frame_tex =
                    Some(ui.ctx().load_texture("organon_viewport", img, egui::TextureOptions::LINEAR))
            }
        }
    }
    state.frame_buf = buf;

    let rect = ui.max_rect();
    theme::paint::well(rect, theme::radius()).into_iter().for_each(|s| {
        ui.painter().add(s);
    });

    match state.frame_tex.as_ref() {
        Some(tex) => {
            // Letterbox: fit the frame inside the pane without distorting it. A stretched
            // viewport misrepresents the scene's proportions, which for a tool whose subject is
            // geometry is not a cosmetic problem.
            let src = tex.size_vec2();
            let scale = (rect.width() / src.x).min(rect.height() / src.y);
            let draw = egui::Rect::from_center_size(rect.center(), src * scale);
            ui.painter().image(
                tex.id(),
                draw,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            let open = state.frame_reader.as_ref().is_some_and(|r| r.is_open());
            let msg = if open {
                "waiting for the first frame…".to_string()
            } else {
                format!(
                    "no frames — open the visual window ({MIRROR_W}×{MIRROR_H} mirror)"
                )
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(11.0),
                theme::MUTED(),
            );
        }
    }
}

/// Presets: each is the full state of every parameter. Save Current snapshots
/// it (auto-named, renameable); clicking a preset recalls it, overwriting
/// everything through the host setter.
/// One row action surfaced by `draw_preset_list` (only one fires per frame).
enum RowAction {
    Recall,
    Rename,
    Delete,
    Update,
    RenameCommit,
    RenameCancel,
}

/// Render one preset list (global or per-tab) as a column of cards and return
/// the single action the user took this frame, if any. Pure UI — the caller
/// owns `PresetUi` and routes the action by scope.
fn draw_preset_list(
    ui: &mut egui::Ui,
    list: &[preset::Preset],
    rename_idx: Option<usize>,
    rename_buf: &mut String,
) -> Option<(usize, RowAction)> {
    let mut out: Option<(usize, RowAction)> = None;
    if list.is_empty() {
        ui.label(egui::RichText::new("None yet.").weak().small());
    }
    for i in 0..list.len() {
        let tile = ui.painter().add(egui::Shape::Noop);
        let framed = theme::card_frame()
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if rename_idx == Some(i) {
                    ui.add(
                        egui::TextEdit::singleline(rename_buf)
                            .desired_width(ui.available_width()),
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("ok").clicked() {
                            out = Some((i, RowAction::RenameCommit));
                        }
                        if ui.small_button("cancel").clicked() {
                            out = Some((i, RowAction::RenameCancel));
                        }
                    });
                } else {
                    let name = list[i].name.clone();
                    if ui
                        .add_sized([ui.available_width(), 22.0], egui::Button::new(name))
                        .on_hover_text("Recall this preset")
                        .clicked()
                    {
                        out = Some((i, RowAction::Recall));
                    }
                    ui.horizontal(|ui| {
                        // #354: compact R / D / U (Rename / Delete / Update).
                        if ui.small_button("R").on_hover_text("Rename").clicked() {
                            out = Some((i, RowAction::Rename));
                        }
                        if ui.small_button("D").on_hover_text("Delete").clicked() {
                            out = Some((i, RowAction::Delete));
                        }
                        if ui
                            .small_button("U")
                            .on_hover_text("Update to the current state")
                            .clicked()
                        {
                            out = Some((i, RowAction::Update));
                        }
                    });
                }
            });
        // #542 T2 §10 — the preset column carries the strongest material gradients in the
        // reference: a satin face plus a genuine diagonal sheen (the one place the diagonal is
        // real rather than emergent), so each tile reads as an individual physical module and
        // the column as a rack-mounted memory bank.
        //
        // NOTE: §10 also specifies a distinct *selected* tile, and this passes the row being
        // renamed as the closest thing we have. Organon has no "last recalled preset" state —
        // recall applies values and returns — so there is nothing else to light. Adding that
        // state is a small follow-up; inventing a fake selection here would be worse than
        // leaving the treatment visibly incomplete.
        ui.painter().set(
            tile,
            theme::preset_tile_chrome(ui, framed.response.rect, rename_idx == Some(i)),
        );
        ui.add_space(4.0);
    }
    out
}

/// Make `desired` unique against `existing` by appending " 2", " 3", … on collision.
fn unique_preset_name(desired: &str, existing: &[String]) -> String {
    if !existing.iter().any(|n| n == desired) {
        return desired.to_string();
    }
    for k in 2.. {
        let cand = format!("{desired} {k}");
        if !existing.iter().any(|n| *n == cand) {
            return cand;
        }
    }
    desired.to_string()
}

/// #425: on save, if auto-naming is on, write the just-saved scene's identity to the
/// request sidecar and bump `name_gen` so the visual asks the local model for a name.
/// The provisional name (`Preset N`) is recorded so the async reply can patch the right
/// preset. Writes the file *before* bumping the counter so the visual never reads a stale
/// or missing request; a failed write leaves the provisional name untouched.
fn emit_name_request(
    state: &mut preset::PresetUi,
    values: &preset::PresetValues,
    scope: preset::PresetScope,
    provisional: &str,
    name_gen: &Arc<AtomicU32>,
) {
    if !state.auto_name_presets {
        return;
    }
    let id = name_gen.load(Ordering::Relaxed).wrapping_add(1).max(1);
    // What kind of preset this is, for the prompt: "Scene", or the tab's label.
    let scope_label = match scope {
        preset::PresetScope::Global => "Scene".to_string(),
        preset::PresetScope::Tab(t) => t.label().to_string(),
    };
    // Existing names in the SAME list (the new preset isn't pushed yet), so the model is
    // told to make something distinct rather than colliding into a "Foo 2" duplicate. Cap to
    // the most recent handful to keep the prompt bounded.
    let mut avoid: Vec<String> = match scope {
        preset::PresetScope::Global => state.presets.iter().map(|p| p.name.clone()).collect(),
        preset::PresetScope::Tab(t) => {
            state.tab_presets[t.index()].iter().map(|p| p.name.clone()).collect()
        }
    };
    avoid.retain(|n| n != provisional);
    if avoid.len() > 16 {
        avoid = avoid.split_off(avoid.len() - 16);
    }
    let req = agent::NameRequest {
        id,
        scope: scope_label,
        features: agent::scene_features(values, scope),
        avoid,
    };
    let Ok(json) = serde_json::to_string(&req) else { return };
    if std::fs::write(ipc::name_request_path(), json).is_ok() {
        name_gen.store(id, Ordering::Relaxed);
        // Saving faster than the visual can name (it overwrites the request file, so only
        // the latest is serviced) would otherwise leak unmatched entries — keep it bounded.
        if state.name_pending.len() >= 64 {
            state.name_pending.remove(0);
        }
        state.name_pending.push((id, scope, provisional.to_string()));
    }
}

/// #425: apply any preset names the visual has produced. Each pending request has its own
/// reply file (`organic-math-namereply-<id>.txt`, holding just the name), so overlapping
/// saves can't clobber one another. Drain every reply that has arrived: consume its file +
/// pending entry, then rename the preset if it still carries its provisional label. Runs
/// every editor frame.
fn drain_name_reply(state: &mut preset::PresetUi) {
    let mut i = 0;
    while i < state.name_pending.len() {
        let id = state.name_pending[i].0;
        let path = ipc::name_reply_path(id);
        let Ok(body) = std::fs::read_to_string(&path) else {
            i += 1;
            continue;
        };
        // Reply present → consume it (delete the file + drop the pending entry) whether or
        // not it actually named anything, so a failed naming can't leave the entry stuck.
        let _ = std::fs::remove_file(&path);
        let (_, scope, provisional) = state.name_pending.remove(i);
        let name = body.lines().next().unwrap_or("").trim().to_string();
        if !name.is_empty() {
            apply_name_reply(state, scope, &provisional, &name);
        }
    }
}

/// Rename the pending preset (matched by its unique provisional label — a manual rename in
/// the meantime wins) to the model's `name`, deduped within its own list, re-pointing any
/// key that referenced the provisional name.
fn apply_name_reply(
    state: &mut preset::PresetUi,
    scope: preset::PresetScope,
    provisional: &str,
    name: &str,
) {
    match scope {
        preset::PresetScope::Global => {
            let existing: Vec<String> = state
                .presets
                .iter()
                .filter(|p| p.name != provisional)
                .map(|p| p.name.clone())
                .collect();
            let final_name = unique_preset_name(name, &existing);
            if let Some(p) = state.presets.iter_mut().find(|p| p.name == provisional) {
                p.name = final_name.clone();
                // Re-point any key mapped to the provisional name (the manual-rename rule).
                for mapped in state.mapping.notes.values_mut() {
                    if *mapped == provisional {
                        *mapped = final_name.clone();
                    }
                }
                state.keymap_dirty = true;
                preset::save(&state.presets);
            }
        }
        preset::PresetScope::Tab(tab) => {
            let existing: Vec<String> = state.tab_presets[tab.index()]
                .iter()
                .filter(|p| p.name != provisional)
                .map(|p| p.name.clone())
                .collect();
            let final_name = unique_preset_name(name, &existing);
            let list = &mut state.tab_presets[tab.index()];
            if let Some(p) = list.iter_mut().find(|p| p.name == provisional) {
                p.name = final_name;
                preset::save_tab(tab, list);
            }
        }
    }
}

fn presets_ui(
    ui: &mut egui::Ui,
    apply_gen: &AtomicU32,
    params: &OrganicMathParams,
    setter: &ParamSetter,
    state: &mut preset::PresetUi,
    hdr_gen: &Arc<AtomicU32>,
    beat_pos: &Arc<AtomicU32>,
    name_gen: &Arc<AtomicU32>,
) {
    use preset::PresetScope;
    // #425: seed auto-naming ON once (PresetUi derives Default = false), and drain any
    // preset name the visual has produced since last frame (the rail is always drawn).
    if !state.auto_name_seeded {
        state.auto_name_seeded = true;
        state.auto_name_presets = true;
    }
    drain_name_reply(state);
    // The preset rail has two tabs (#145): the full-state Global list, and the
    // list for the currently-selected editor tab (Generator / Motion / Look).
    // The Environment and Settings UI tabs hold per-display state that presets
    // don't capture, so they have no per-tab list — the rail falls back to
    // Global while one of them is active.
    let active_tab = state.tab.preset_tab();
    if active_tab.is_none() {
        state.rail_tab = preset::PresetRailTab::Global;
    }
    ui.horizontal(|ui| {
        // #354: "Global" is now "Scene" (Generator + Motion + Environment + Look).
        ui.selectable_value(&mut state.rail_tab, preset::PresetRailTab::Global, "Scene");
        if let Some(tab) = active_tab {
            ui.selectable_value(&mut state.rail_tab, preset::PresetRailTab::Tab, tab.label());
        }
    });
    ui.separator();

    // Actions are collected, then applied after the immutable list borrows end.
    let mut recall: Option<(PresetScope, preset::PresetValues)> = None;
    // Delete/Update route through a confirm (#354): (scope, index, kind).
    let mut request: Option<(PresetScope, usize, preset::ConfirmKind)> = None;
    let mut start_rename: Option<(PresetScope, usize)> = None;
    let mut commit_rename = false;
    let mut cancel_rename = false;

    match state.rail_tab {
        // --- Scene presets (Generator + Motion + Environment + Look) ---
        preset::PresetRailTab::Global => {
            if ui
                .add_sized([ui.available_width(), 24.0], egui::Button::new("＋ Save Scene"))
                .on_hover_text("Save the Scene (generator + motion + environment + look)")
                .clicked()
            {
                // A UNIQUE provisional label (not the bare "Preset {len+1}", which can
                // collide with an existing preset after a middle delete): the async name
                // reply is matched back to its preset by this label, so a duplicate would
                // let the AI name land on the wrong row.
                let existing: Vec<String> =
                    state.presets.iter().map(|p| p.name.clone()).collect();
                let provisional =
                    unique_preset_name(&format!("Preset {}", state.presets.len() + 1), &existing);
                let values = preset::PresetValues::capture(params);
                emit_name_request(state, &values, PresetScope::Global, &provisional, name_gen);
                state.presets.push(preset::Preset { name: provisional, values });
                preset::save(&state.presets);
                state.keymap_dirty = true; // a new preset may already be referenced by a key
            }
            ui.add_space(6.0);
            let g_rename = if state.rename_scope == PresetScope::Global {
                state.rename_idx
            } else {
                None
            };
            let g_action =
                draw_preset_list(ui, &state.presets, g_rename, &mut state.rename_buf);
            if let Some((i, act)) = g_action {
                match act {
                    RowAction::Recall => {
                        recall = Some((PresetScope::Global, state.presets[i].values.clone()))
                    }
                    RowAction::Rename => start_rename = Some((PresetScope::Global, i)),
                    RowAction::Delete => {
                        request = Some((PresetScope::Global, i, preset::ConfirmKind::Delete))
                    }
                    RowAction::Update => {
                        request = Some((PresetScope::Global, i, preset::ConfirmKind::Update))
                    }
                    RowAction::RenameCommit => commit_rename = true,
                    RowAction::RenameCancel => cancel_rename = true,
                }
            }
        }
        // --- Active-tab presets (override only this tab) ---
        // Unreachable for Environment/Settings: the rail is forced to Global
        // above when the active UI tab has no preset partition.
        preset::PresetRailTab::Tab => {
            if let Some(active_tab) = active_tab {
            let scope = PresetScope::Tab(active_tab);
            if ui
                .add_sized(
                    [ui.available_width(), 24.0],
                    egui::Button::new(format!("＋ Save {}", active_tab.label())),
                )
                .on_hover_text("Save only this tab's parameters as a tab preset")
                .clicked()
            {
                // Unique provisional label (see the Scene save above) so the async name
                // reply can't be matched onto the wrong preset.
                let existing: Vec<String> = state.tab_presets[active_tab.index()]
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                let provisional = unique_preset_name(
                    &format!("{} {}", active_tab.label(), existing.len() + 1),
                    &existing,
                );
                let values = preset::PresetValues::capture(params);
                emit_name_request(
                    state,
                    &values,
                    PresetScope::Tab(active_tab),
                    &provisional,
                    name_gen,
                );
                let list = &mut state.tab_presets[active_tab.index()];
                list.push(preset::Preset { name: provisional, values });
                preset::save_tab(active_tab, list);
            }
            help(ui, "Recall overrides only this tab's params; the other tabs stay put.");
            ui.add_space(6.0);
            let t_rename = if state.rename_scope == scope {
                state.rename_idx
            } else {
                None
            };
            let t_action = draw_preset_list(
                ui,
                &state.tab_presets[active_tab.index()],
                t_rename,
                &mut state.rename_buf,
            );
            if let Some((i, act)) = t_action {
                match act {
                    RowAction::Recall => {
                        recall = Some((scope, state.tab_presets[active_tab.index()][i].values.clone()))
                    }
                    RowAction::Rename => start_rename = Some((scope, i)),
                    RowAction::Delete => request = Some((scope, i, preset::ConfirmKind::Delete)),
                    RowAction::Update => request = Some((scope, i, preset::ConfirmKind::Update)),
                    RowAction::RenameCommit => commit_rename = true,
                    RowAction::RenameCancel => cancel_rename = true,
                }
            }
            } // end if let Some(active_tab)
        }
    }

    // --- Apply collected actions ---
    if let Some(r) = request {
        state.confirm = Some(r);
    }
    if let Some((scope, v)) = recall {
        // #354: beat-quantized recall — Scene → the Scene-timing dropdown,
        // Scene-component tabs → the Component-timing dropdown, Audio/Synth/
        // Settings always instant. Shared with the #356 controller drain.
        enqueue_recall(state, scope, v, apply_gen, params, setter, hdr_gen, beat_pos);
    }
    // Fire a scheduled recall when its boundary arrives. This runs every editor
    // frame (the presets rail is always drawn), so it also fires recalls the
    // controller mailbox queued.
    poll_pending_recall(
        state,
        apply_gen,
        params,
        setter,
        hdr_gen,
        beat_pos,
        Some(ui.ctx()),
    );
    if let Some((scope, i)) = start_rename {
        state.rename_scope = scope;
        state.rename_buf = match scope {
            PresetScope::Global => state.presets[i].name.clone(),
            PresetScope::Tab(tab) => state.tab_presets[tab.index()][i].name.clone(),
        };
        state.rename_idx = Some(i);
    }
    if commit_rename {
        if let Some(i) = state.rename_idx {
            let new = state.rename_buf.trim().to_string();
            match state.rename_scope {
                PresetScope::Global => {
                    let old = state.presets[i].name.clone();
                    state.presets[i].name = new.clone();
                    // Re-point any keys mapped to the old name so the mapping
                    // follows the rename instead of orphaning.
                    for name in state.mapping.notes.values_mut() {
                        if *name == old {
                            *name = new.clone();
                        }
                    }
                    preset::save(&state.presets);
                    state.keymap_dirty = true;
                }
                PresetScope::Tab(tab) => {
                    state.tab_presets[tab.index()][i].name = new;
                    preset::save_tab(tab, &state.tab_presets[tab.index()]);
                }
            }
        }
        state.rename_idx = None;
    }
    if cancel_rename {
        state.rename_idx = None;
    }

    // --- Quick "Are you sure?" confirm for Update / Delete (#354), Yes-default ---
    if let Some((scope, i, kind)) = state.confirm {
        // Guard against a stale index (list changed under us).
        let valid = match scope {
            PresetScope::Global => i < state.presets.len(),
            PresetScope::Tab(tab) => i < state.tab_presets[tab.index()].len(),
        };
        if !valid {
            state.confirm = None;
        } else {
            let title = match kind {
                preset::ConfirmKind::Update => "Update this preset to the current state?",
                preset::ConfirmKind::Delete => "Delete this preset?",
            };
            let mut close = false;
            let mut do_it = false;
            egui::Window::new(title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        let yes = ui.button("Yes");
                        yes.request_focus(); // default to Yes
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if yes.clicked() || enter {
                            do_it = true;
                            close = true;
                        }
                        if ui.button("No").clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            close = true;
                        }
                    });
                });
            if do_it {
                match (kind, scope) {
                    (preset::ConfirmKind::Update, PresetScope::Global) => {
                        state.presets[i].values = preset::PresetValues::capture(params);
                        preset::save(&state.presets);
                        // A Key Map note may point at this preset — rebuild so held
                        // MIDI notes serve the updated look, not the stale snapshot.
                        state.keymap_dirty = true;
                    }
                    (preset::ConfirmKind::Update, PresetScope::Tab(tab)) => {
                        state.tab_presets[tab.index()][i].values =
                            preset::PresetValues::capture(params);
                        preset::save_tab(tab, &state.tab_presets[tab.index()]);
                        state.keymap_dirty = true;
                    }
                    (preset::ConfirmKind::Delete, PresetScope::Global) => {
                        state.presets.remove(i);
                        preset::save(&state.presets);
                        state.keymap_dirty = true; // a deleted preset's keys go inert
                        if state.rename_scope == scope && state.rename_idx == Some(i) {
                            state.rename_idx = None;
                        }
                    }
                    (preset::ConfirmKind::Delete, PresetScope::Tab(tab)) => {
                        state.tab_presets[tab.index()].remove(i);
                        preset::save_tab(tab, &state.tab_presets[tab.index()]);
                        if state.rename_scope == scope && state.rename_idx == Some(i) {
                            state.rename_idx = None;
                        }
                    }
                }
            }
            if close {
                state.confirm = None;
            }
        }
    }
}

/// Truncate a label to `n` characters (char-safe), appending an ellipsis if cut.
fn short(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    } else {
        s.to_string()
    }
}

/// The Key Map window: a one-octave piano (paged C0..C5) where right-clicking a
/// key assigns a preset. A mapped key tints amber; the live-held key lights up.
fn keymap_ui(
    ui: &mut egui::Ui,
    state: &mut preset::PresetUi,
    active_note: &AtomicU8,
) {
    let preset::PresetUi { presets, mapping, keymap_octave, keymap_dirty, .. } = state;
    let active = active_note.load(Ordering::Relaxed);

    // --- Header: octave paging ---
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*keymap_octave > keymap::MIN_OCTAVE, egui::Button::new("◀"))
            .clicked()
        {
            *keymap_octave -= 1;
        }
        let base = keymap::midi_for(*keymap_octave, 0);
        ui.label(
            egui::RichText::new(format!("Octave  C{}  (MIDI {}–{})", keymap_octave, base, base + 11))
                .color(theme::BONE())
                .strong(),
        );
        if ui
            .add_enabled(*keymap_octave < keymap::MAX_OCTAVE, egui::Button::new("▶"))
            .clicked()
        {
            *keymap_octave += 1;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Clear all")
                .on_hover_text("Remove every key→preset assignment")
                .clicked()
            {
                mapping.notes.clear();
                *keymap_dirty = true;
            }
            ui.label(
                egui::RichText::new(format!("{} mapped", mapping.notes.len())).weak(),
            );
        });
    });
    ui.add_space(4.0);

    if presets.is_empty() {
        ui.label(
            egui::RichText::new("No presets yet — save one first, then right-click a key to assign it.")
                .weak(),
        );
    } else {
        ui.label(
            egui::RichText::new("Right-click a key to assign a preset. Holding that MIDI note activates the preset on the visual.")
                .weak()
                .small(),
        );
    }
    ui.add_space(6.0);

    // --- Keyboard geometry ---
    const WHITE_PC: [i32; 7] = [0, 2, 4, 5, 7, 9, 11]; // C D E F G A B
    // (white-index this black sits to the right of, its pitch class)
    const BLACK: [(usize, i32); 5] = [(0, 1), (1, 3), (3, 6), (4, 8), (5, 10)];

    let w = ui.available_width().clamp(360.0, 720.0);
    let h = 150.0;
    let (resp, painter) = ui.allocate_painter(egui::vec2(w, h), egui::Sense::hover());
    let area = resp.rect;
    let ww = area.width() / 7.0;
    let bh = h * 0.60; // black-key / white-upper height
    let base_midi = keymap::midi_for(*keymap_octave, 0);

    let white_base = egui::Color32::from_rgb(232, 233, 240);
    let white_assigned = egui::Color32::from_rgb(255, 214, 156);
    let black_base = egui::Color32::from_rgb(28, 30, 42);
    let black_assigned = egui::Color32::from_rgb(150, 112, 48);
    let outline = egui::Color32::from_rgb(60, 64, 80);

    enum KeyAction {
        Assign(u8, String),
        Clear(u8),
    }
    let mut action: Option<KeyAction> = None;

    // Draw + interact one key. `hit` is the clickable sub-rect (whites use only
    // their lower strip so they never overlap the black keys above them).
    let mut do_key =
        |ui: &egui::Ui, painter: &egui::Painter, midi: i32, draw: egui::Rect, hit: egui::Rect, is_black: bool| {
            if !(0..=127).contains(&midi) {
                return;
            }
            let note = midi as u8;
            let assigned = mapping.get(note).map(|s| s.to_string());
            let is_active = active == note;
            let fill = if is_active {
                ACCENT()
            } else if assigned.is_some() {
                if is_black { black_assigned } else { white_assigned }
            } else if is_black {
                black_base
            } else {
                white_base
            };
            painter.rect_filled(draw, egui::CornerRadius::same(2), fill);
            painter.rect_stroke(
                draw,
                egui::CornerRadius::same(2),
                egui::Stroke::new(1.0, outline),
                egui::StrokeKind::Inside,
            );
            // Labels on white keys: note name + (truncated) assigned preset.
            if !is_black {
                let text_col = if is_active {
                    egui::Color32::from_rgb(20, 20, 28)
                } else {
                    egui::Color32::from_rgb(70, 74, 90)
                };
                painter.text(
                    egui::pos2(draw.center().x, draw.max.y - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    keymap::note_name(note),
                    egui::FontId::proportional(11.0),
                    text_col,
                );
                if let Some(name) = &assigned {
                    painter.text(
                        egui::pos2(draw.center().x, draw.max.y - 18.0),
                        egui::Align2::CENTER_BOTTOM,
                        short(name, 8),
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_rgb(120, 70, 10),
                    );
                }
            }

            let kr = ui.interact(hit, ui_id_for(note), egui::Sense::click());
            let kr = kr.on_hover_text(match &assigned {
                Some(n) => format!("{}  →  {}", keymap::note_name(note), n),
                None => format!("{}  (unassigned)", keymap::note_name(note)),
            });
            kr.context_menu(|menu| {
                menu.label(
                    egui::RichText::new(format!("Assign to {}", keymap::note_name(note))).strong(),
                );
                if assigned.is_some() && menu.button("✕  Clear").clicked() {
                    action = Some(KeyAction::Clear(note));
                    menu.close_menu();
                }
                menu.separator();
                if presets.is_empty() {
                    menu.label(egui::RichText::new("No presets saved").weak());
                }
                egui::ScrollArea::vertical().max_height(260.0).show(menu, |menu| {
                    for p in presets.iter() {
                        let selected = assigned.as_deref() == Some(p.name.as_str());
                        if menu.selectable_label(selected, &p.name).clicked() {
                            action = Some(KeyAction::Assign(note, p.name.clone()));
                            menu.close_menu();
                        }
                    }
                });
            });
        };

    // White keys (full height; clickable on their lower strip).
    for (i, &pc) in WHITE_PC.iter().enumerate() {
        let x0 = area.min.x + i as f32 * ww;
        let draw = egui::Rect::from_min_max(
            egui::pos2(x0, area.min.y),
            egui::pos2(x0 + ww, area.max.y),
        );
        let hit = egui::Rect::from_min_max(
            egui::pos2(x0, area.min.y + bh),
            egui::pos2(x0 + ww, area.max.y),
        );
        do_key(ui, &painter, base_midi + pc, draw, hit, false);
    }
    // Black keys (top, drawn over the white boundaries).
    let bw = ww * 0.62;
    for &(wi, pc) in BLACK.iter() {
        let cx = area.min.x + (wi as f32 + 1.0) * ww;
        let draw = egui::Rect::from_min_max(
            egui::pos2(cx - bw * 0.5, area.min.y),
            egui::pos2(cx + bw * 0.5, area.min.y + bh),
        );
        do_key(ui, &painter, base_midi + pc, draw, draw, true);
    }

    // Apply the chosen assignment (deferred so the closures above only read).
    match action {
        Some(KeyAction::Assign(note, name)) => {
            mapping.assign(note, &name);
            *keymap_dirty = true;
        }
        Some(KeyAction::Clear(note)) => {
            mapping.clear(note);
            *keymap_dirty = true;
        }
        None => {}
    }
}

/// Stable per-note egui id for the key widgets.
fn ui_id_for(note: u8) -> egui::Id {
    egui::Id::new(("om_key", note))
}

impl ClapPlugin for OrganicMath {
    const CLAP_ID: &'static str = "com.amplifyluxury.organic-math";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Parametric 3D generative visualizer — control surface for the Organon visual");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::Utility, ClapFeature::Stereo];
}

impl Vst3Plugin for OrganicMath {
    const VST3_CLASS_ID: [u8; 16] = *b"OrganicMathViz01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Tools];
}

nih_export_clap!(OrganicMath);
nih_export_vst3!(OrganicMath);

//! **The world** — the renderer and everything that drives it, as a *library* module tree
//! (#572, the hoist).
//!
//! # Why this module exists
//!
//! Everything that draws used to live in `bin/visual.rs`: `render.rs` and its GPU siblings
//! were `#[path]`-included **by the binary**, which meant only the binary could ever call
//! them. That was the right shape while the visual was the only thing that rendered.
//!
//! [#572] route C changes that: Organon Mind's editor becomes a wgpu surface built on the
//! window nih-plug hands it, drawing the scene *and* the interface on one device. That editor
//! lives in `lib.rs`, so the library has to be able to reach the renderer — a binary's modules
//! are unreachable from the library that binary depends on.
//!
//! Being filled in three stages so each step is reviewable on its own:
//!
//! 1. **the module tree** (#574) — the renderer compiles as a library module,
//! 2. **`World`** (*this change*) — `bin/visual.rs`'s `App`, moved here,
//! 3. **the window seam** — the binary keeps winit and hands in a target per frame.
//!
//! # Stage 2: the orphan rule
//!
//! [`World`] **is** the old `App`, renamed, with its ~12 000 lines carried over verbatim.
//!
//! `ApplicationHandler` is winit's trait and `World` is this crate's *library* type, so
//! `bin/visual.rs` cannot implement it on `World` and owns a thin `VisualApp { world: World }`
//! wrapper instead. Rather than make ~20 fields (`overlay_on`, `pathtrace_on`, `frame_guide`,
//! the recorder latches, `ui.visible`, the camera offsets, …) `pub` so a handler in the binary
//! could reach them, the handler *body* moved here with everything it touches and the binary
//! forwards a handful of calls.
//!
//! The one thing that could not simply move is `event_loop.exit()`: the event loop belongs to
//! the binary. [`on_window_event`](World::on_window_event) returns an [`EventResponse`] instead
//! and the binary acts on it.
//!
//! # Stage 3: the window seam
//!
//! The world no longer owns a window, a surface, or a swapchain. `Gfx` is device-side only; the
//! host acquires an image, hands the texture in on a [`FrameTarget`], and presents afterwards.
//! Everything that used to be a window *side effect* inside the frame is either a fact the
//! caller states (size, format, `presented`, EDR headroom, wide gamut) or a
//! [`FrameRequests`] the caller applies (title, inner size). See [`Gfx`] for the before/after
//! table.
//!
//! That is what makes route C possible: the editor's surface is built on the `NSView` nih-plug
//! hands it, so there is no winit window in that process at all — and now the world does not ask
//! for one.
//!
//! # The last winit coupling, and its removal (#593 Tier 3)
//!
//! Stage 3 left exactly one: `FrameTarget::ui_window` was still a `&winit::Window`, because
//! `ui_layer` translated input through `egui-winit`. Tier 3 replaced it. `ui_layer` is now
//! generic over [`EguiPlatform`](crate::egui_platform::EguiPlatform) — the winit
//! arm is `winit_platform`, the baseview arm is route C's — and the frame states
//! [`FrameTarget::ui_scale_factor`] instead of handing over a window.
//!
//! **`winit::window::Window` no longer appears in this file.** `winit::event::WindowEvent` does,
//! in [`on_window_event`](World::on_window_event), and that is deliberate: it is the *winit
//! host's* entry point, carrying the visual's keymap. A baseview host never calls it.
//!
//! # ✅ THIS FILE IS COMPILED ONCE (organon#49 T4c-ii) — the old duality is retired
//!
//! It used to be compiled **twice**: once as `organic-math-native`'s cfg-gated `pub mod
//! world`, and once as a `#[path]` module of `bin/visual.rs`. That was not redundancy, it
//! was the mechanism — a `#[path]` include is not a cargo feature, so including the *source*
//! was how the visual binary got a world the shipping cdylib did not get.
//!
//! T4c-ii replaced the mechanism rather than the intent: this file lives in `organon-world`
//! behind its `world` feature, and the visual is `organon-visual`, a package of its own that
//! turns the feature on. The plugin crate leaves it off except under `mind-edition` /
//! `console-edition`. Same gate, one compilation.
//!
//! **What that retires, concretely.** Nothing here needs a spelling that resolves in two
//! different crate roots any more. `render.rs`'s `super::axes` / `super::chamber` are now
//! plain siblings inside `organon-render`; the `use organon_core::math` shim that had to sit
//! at `bin/visual.rs`'s root — load-bearing only because `crate::` meant *the binary* there —
//! is gone. If you are about to preserve a spelling "because this file is compiled twice",
//! check first: it is not.
//!
//! # Why this whole module is behind the `world` feature
//!
//! Measured, not assumed. Ungated, `pub mod world` grew the plugin cdylib from 12 749 728 to
//! 13 250 704 bytes (+490 KB), with **zero** wgpu/naga dynamic symbols either way — nothing new
//! becomes *reachable*, but a shipping VST3 that changes size for no user-visible reason is
//! exactly what "full Organon is untouched" rules out. Organon Mind ships no VST3 (#483), so
//! under `mind-edition` the growth costs nothing. The gate also keeps `ui_layer`'s egui-wgpu
//! stack (and a GPU device) out of Ableton's process in a default build, which is the
//! constraint #554 wrote down.
//!
//! [#572]: https://github.com/organonart/organon/issues/572

// #626 Tier 4 — these three moved to the `organon-render` crate and are re-exported here,
// so every `render::…` / `axes::…` / `chamber::…` call site below still resolves verbatim.
//
// ⚠️ These were `#[path]` module declarations, and `axes`/`chamber` had to be declared
// *before* `render` because `render.rs` reaches them through `super::`. **That ordering
// constraint is gone** — a `pub use` does not participate in `#[path]` resolution, and the
// `super::axes` / `super::chamber` paths now resolve inside `organon-render` itself, where
// its `lib.rs` declares all three. Order here is presentational only; don't preserve it out
// of a caution that no longer applies.
//
// Why the move was worth it: `world.rs` is `#[path]`-included by BOTH `lib.rs` and
// `bin/visual.rs` (a binary's `#[path]` modules are unreachable from the library it depends
// on), so the renderer was compiled twice in a Mind build. As a crate it compiles once.

/// Capture decoration (#135 Phase 5): 3-D axes + wireframe box line pass.
pub use organon_render::axes;

/// Field Chamber (#346): analyzer panels (oscilloscope + spectrum) on the box back walls.
pub use organon_render::chamber;

/// The renderer proper: `Renderer`, `RenderFrame`, `RenderPath`, and every pass.
pub use organon_render::render;

// Capture / production frame (#135): fixed-resolution offscreen target + letterbox blit.
#[path = "capture.rs"]
pub mod capture;

// Capture overlay (#135 Phase 2): title / formula / live readouts text pass.
#[path = "overlay.rs"]
pub mod overlay;

// Hardware ray tracing Tier 0 (#195): BLAS/TLAS plumbing + the debug view.
#[path = "rt.rs"]
pub mod rt;
#[path = "metal_island.rs"]
pub mod metal_island;

// GPU frame timing via timestamp queries (#277 Tier 3): the performance status
// bar's true GPU-ms headroom figure. No-op (None) without TIMESTAMP_QUERY.
#[path = "gpu_timer.rs"]
pub mod gpu_timer;

// In-app production recorder (#430): read the production texture back + pipe to ffmpeg.
#[path = "recorder.rs"]
pub mod recorder;

// #452 Tier 3 ("the eyes"): single-frame PNG readback of the production texture, so an
// external agent (Bianca) can `set` → `snap` → judge → `set`. Visual-only, like recorder.
#[path = "snap.rs"]
pub mod snap;

// #554 Tier 4: egui drawn on THIS process's wgpu device, over the real scene.
//
// It used to be declared by `bin/visual.rs` specifically so that egui-wgpu and a GPU device
// could never reach the plugin cdylib — i.e. Ableton's process — which is what #554 said must
// not happen. That guarantee is now the `mind-edition` gate on this whole module rather than a
// property of *where* the module is declared: a default build still compiles `ui_layer` only
// into the visual binary, and Organon Mind ships no VST3, so nothing egui-shaped ever enters a
// host. Under `--features mind-edition` the library does take it, which is the point — route
// C's editor is this layer.
#[path = "ui_layer.rs"]
pub mod ui_layer;

// #593 Tier 3: the winit arm of the egui platform seam — `egui-winit` behind
// `egui_platform::EguiPlatform`. It lives with `ui_layer` and under the same gate, because
// `egui-winit` is the *visual binary's* dependency; the trait itself is ungated in `lib.rs`,
// where route C's baseview arm can reach it.
#[path = "winit_platform.rs"]
pub mod winit_platform;

use glam::{DVec3, Mat3, Mat4, Vec2, Vec3, Vec4};
// #593 Tier 4 — the frame mirror is full Organon's path only, so the module it lives in is
// `#[cfg(not(mind-edition))]` in `lib.rs` and everything here that touches it is gated the same
// way. Note this file is compiled **twice**: as the library's `world` (mind-edition only, the
// one Mind's wgpu editor drives — where the mirror is therefore always absent) and as
// `bin/visual.rs`'s `#[path]` copy, which ships in both editions and keeps the mirror in the
// default build that produces full Organon's projector.
#[cfg(not(feature = "mind-edition"))]
use crate::frame_ring;
use organon_core::ipc;
use organon_agent as agent;
use organon_agent::ChatClient; // bring the trait into scope for client.complete()
use organon_core::math;
use organon_mind::mind_log;
use organon_mind::mind_ring;
// organon#217 T1 — the glyph ring: a text-effect cell grid rendered as lit tiles.
use organon_core::glyph_ring;
// #554 Tier 4 — the routing verdict for every pointer event (see `ui_layer`).
use organon_mind::mind_shell::PointerTarget;
// #621 — the camera's backend-neutral input. In the library it is `crate::scene_input`; in
// `bin/visual.rs`'s `#[path]` copy of this file it is the same module reached through the crate
// name, which is why the import is spelled this way like every other one above.
//
// Only `CameraInput` crosses. There is deliberately **no** `World::apply_scene_gesture` helper:
// this file is compiled twice, and a per-frame `SceneGesture` drain has a caller in exactly one
// of those compilations — it would be permanently dead in the visual's, which is how a
// blanket `allow(dead_code)` gets added and then stops reporting a reader that really did go
// away. The editor loops over `SceneGesture::inputs()` at its own call site instead.
use crate::scene_input;
use crate::scene_input::CameraInput;
use organon_scene::overlay_meta;
use organon_core::params::{BoidsForm, FuncName, GeneratorMode, OscDivision, ParamValues};
use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key, NamedKey};
// #593 Tier 3 — the window's geometry as data, which is what replaced `&winit::Window` here.
use crate::egui_platform::WindowGeometry;


// No node cap: the field builds every x·y·z·q node the params ask for. The GPU
// instance buffer grows to fit (see `render.rs`), so the only limit is memory /
// frame time. (`draw_tissue` still takes a ceiling arg — pass the max so its
// guard never trips — in case we want to reintroduce a sane cap later.)
const CUBE_CEILING: usize = usize::MAX;

/// The graphics state — **device-side only** since #572 stage 3 (the window seam).
///
/// There used to be a `win: Option<WindowSurface>` here holding the winit window, its wgpu
/// surface, the swapchain config, and the two formats the HDR toggle swaps between. All of it
/// now lives in whoever *owns* the window (`bin/visual.rs`'s `VisualApp`), and the world learns
/// what it needs per frame from [`FrameTarget`]. What remains is the same for every host:
/// device, queue, renderer, capture, overlay, RT, timers, UI layer.
///
/// **Why that mattered enough to do.** Route C's editor builds a wgpu surface on the view
/// nih-plug hands it — there is no winit window anywhere in that process. While the world owned
/// a `WindowSurface`, the editor could not drive it at all. Now the *caller* acquires an image
/// and passes the texture in; drawing is identical either way, and the five in-frame couplings
/// #541 S2 T3 enumerated are gone rather than merely gated:
///
/// | was | is now |
/// |---|---|
/// | `Gfx::window_output` → `resolve_output` | the caller states size + format on the target |
/// | `Window::request_inner_size` | returned in [`FrameRequests::inner_size`] |
/// | swapchain acquire / reconfigure / present | the caller's, bracketing `render_into` |
/// | the render-resolution window title | returned in [`FrameRequests::title`] |
/// | `hdr_macos::set_edr` → `self.hdr.hdr_max` | the caller measures and passes `hdr_max` |
///
/// **The one coupling stage 3 left** — `FrameTarget::ui_window`, a `&winit::Window` kept only
/// because `ui_layer`'s input translation was `egui-winit` — **is gone as of #593 Tier 3.** The
/// layer takes input through `egui_platform::EguiPlatform`, and the frame states
/// [`FrameTarget::ui_scale_factor`] instead of lending a window.
struct Gfx {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: render::Renderer,
    /// The colour format `renderer`'s composite / FX / temporal passes are
    /// currently built for. The swapchain's format (flipped by the HDR toggle)
    /// and an offscreen target's texture format are independent choices, so each
    /// frame rebuilds those pipelines when its *output* format differs from this
    /// (see `render_into`). Everything else (`capture`, `overlay`, `rt`) already
    /// self-heals per call.
    out_format: wgpu::TextureFormat,
    // Capture / production frame (#135): owns the fixed-res offscreen target + the
    // letterbox blit pipeline. Inert while aspect = Native.
    capture: capture::Capture,
    // Capture overlay (#135 Phase 2): glyph atlas + formula textures + text pass.
    overlay: overlay::Overlay,
    // Hardware RT Tier 0 (#195): acceleration structures + the debug pass.
    // `None` when the device lacks ray-query support (non-Metal / pre-M3) —
    // every use site no-ops through it, and the editor greys the card out via
    // `Feedback.rt_available`.
    rt: Option<rt::RtContext>,
    // Neural acceleration detection (#200 Tier 2): whether the adapter offers the
    // cooperative-matrix (simdgroup/tensor) fast path + f16. Detection only — the
    // features are NOT enabled on this device yet; the editor reports them so the
    // Mac knows what a coop-matrix MLP path (+ its GFLOPs benchmark) would ride.
    coopmat_available: bool,
    f16_available: bool,
    // Metal interop island (#200 Tier 3): the startup probe result (dark until the
    // on-Mac objc2-metal `imp` lands). Reported to the editor via Feedback.
    island: metal_island::IslandProbe,
    // GPU frame timer (#277 Tier 3): timestamp-query GPU ms for the status bar.
    // `None` when the device lacks TIMESTAMP_QUERY (the editor shows "n/a").
    gpu_timer: Option<gpu_timer::GpuTimer>,
    // #554 Tier 4: egui on THIS device, drawn over the scene after the composite.
    // `None` for a windowless world — egui's winit input translation needs a window,
    // and an offscreen frame is a picture of the scene, not of the interface.
    ui: Option<ui_layer::UiLayer>,
}

/// Where a frame is being drawn (#541 S2 T3 → #572 stage 3, the window seam).
///
/// It used to be an enum — `Window` (acquire from our own surface and present) or `Offscreen`.
/// Stage 3 collapsed that: **there is only a texture**, and the caller says what it is. A
/// swapchain image and an egui pane's texture are now the same call, which is what lets an
/// editor that owns no winit window drive the world.
///
/// The texture must be `RENDER_ATTACHMENT` at exactly `size` / `format` — the final composite,
/// the letterbox blit, the overlay, the HUDs and the RT debug view all write straight into it.
pub struct FrameTarget<'a> {
    pub texture: &'a wgpu::Texture,
    pub size: (u32, u32),
    pub format: wgpu::TextureFormat,
    /// True when the caller will **present** this texture to a display, false when it owns the
    /// texture for its own purposes. The whole point of the flag is that a presented frame may
    /// use display-negotiated properties (EDR headroom, a Rec.2020 tag) and an offscreen one
    /// must not — see [`frame_hdr_max`] / [`frame_gamut`].
    pub presented: bool,
    /// Display EDR headroom, **measured by the caller** off its own layer. `1.0` = SDR.
    /// Ignored unless `presented`, so an offscreen caller can leave it at `1.0`.
    ///
    /// The world used to measure this itself via `hdr_macos::set_edr`, which reaches for the
    /// `CAMetalLayer` wgpu put on the `NSView`. An offscreen texture has no layer and no
    /// display; making the number an *input* is what removes the temptation to ask.
    pub hdr_max: f32,
    /// Whether the caller tagged its surface Rec.2020 (#119). Same rule: only meaningful for a
    /// presented frame.
    pub wide_gamut: bool,
    /// The display's scale factor, for the interface drawn over this frame. **`None` = draw no
    /// interface**, which is what every offscreen consumer wants (the frame mirror and the
    /// production recorder both want the scene, not a UI painted over it).
    ///
    /// **This field is what the world's last winit coupling turned into** (#593 Tier 3). It was
    /// `ui_window: Option<&'a winit::Window>` — lent purely so `egui-winit` could ask it for
    /// `inner_size()` and `scale_factor()`. `baseview::Window` can answer neither, so the frame
    /// states the geometry instead: the size is already `size`, and this is the other half.
    /// Together they make the `egui_platform::WindowGeometry` the UI layer runs on.
    pub ui_scale_factor: Option<f32>,
}

/// What a frame wants its host to do afterwards (#572 stage 3).
///
/// Two side effects used to be performed *inside* the frame, straight onto the winit window.
/// They are now reported instead, so the world can produce them with no window in reach and the
/// host applies them (or ignores them, as an offscreen host does).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct FrameRequests {
    /// "Lock window to output" (#135): the capture path wants the window resized to the
    /// production size so what you see is what is encoded. Advisory — a host with no resizable
    /// window, or none at all, simply drops it. The request re-fires while the sizes differ, so
    /// ignoring it is stable rather than sticky.
    pub inner_size: Option<(u32, u32)>,
    /// The render-resolution title readout. `Some` only on the frames where it changed, so a
    /// host can set it unconditionally without thrashing.
    pub title: Option<String>,
}

/// The window-independent description of a frame's destination: how big it is,
/// what colour format it is, and whether it is a *presented* swapchain image.
///
/// `presented` is the one bit the ~7k-line frame body needs in order to know
/// whether the window-only side effects apply (present, retitle, lock-window-to-
/// output, EDR headroom). Everything about *drawing* is the same either way.
#[derive(Clone, Copy, PartialEq, Debug)]
struct FrameOutput {
    size: (u32, u32),
    format: wgpu::TextureFormat,
    presented: bool,
}

/// Release a staging buffer's mapping. Split out only so the call site reads as "we are done
/// with the mapped range" rather than as a bare side effect between two copies.
///
/// #593 Tier 4 — `pump_mirror` is its only caller, so it is gated with it. (Left ungated it is
/// the mind-edition build's one new dead-code warning, which on this thread is a defect report,
/// not noise.)
#[cfg(not(feature = "mind-edition"))]
fn drop_mapped(buf: &wgpu::Buffer) {
    buf.unmap();
}

/// #554 Tier 1 — the frame mirror: everything needed to render one extra offscreen frame and
/// hand it to the editor.
///
/// # The cost, stated plainly
///
/// This renders a **second, complete frame** — the window's, then the mirror's. There is no way
/// to avoid that at Tier 1: a swapchain image is not `COPY_SRC`, so the presented frame cannot
/// be read back, and blitting the composite into two destinations means restructuring the pass
/// that #549 just finished restructuring.
///
/// What makes it affordable is **pacing**: [`MIRROR_EVERY`] publishes one mirror frame per N
/// window frames, so at 60 fps the extra load is ~1/N of a frame rather than a doubling. The
/// readback also blocks on the GPU (`poll(Wait)`), which at ~15 Hz is a stall the frame budget
/// absorbs and at 60 Hz would not be. Both are #554 Tier 3's business; neither is a reason to
/// withhold a working viewport.
///
/// **#593 Tier 4 — full Organon only.** Mind's editor renders this world straight into its own
/// window's surface, so there is nothing left to photograph for; the cost above is one nobody on
/// that path should pay and, gated, cannot.
#[cfg(not(feature = "mind-edition"))]
struct Mirror {
    writer: frame_ring::FrameRingWriter,
    /// Render target. `RENDER_ATTACHMENT` because `render_to_texture` composites into it,
    /// `COPY_SRC` because we read it back.
    texture: wgpu::Texture,
    /// Staging buffer for the readback, sized for the *padded* row stride.
    staging: wgpu::Buffer,
    /// Row stride wgpu requires (`MIRROR_W × 4` rounded up to 256).
    padded_bpr: u32,
    /// Tightly-packed RGBA handed to the ring, reused so a publish does not allocate.
    cpu: Vec<u8>,
}

/// Window frames per published mirror frame. `4` → ~15 Hz against a 60 fps window: fast enough
/// to read as live motion, slow enough that the extra frame and its readback stall stay well
/// inside the budget.
#[cfg(not(feature = "mind-edition"))]
const MIRROR_EVERY: u32 = 4;

/// The mirror renders SDR 8-bit sRGB. `render_into` already forces an offscreen frame to
/// composite SDR (`FrameOutput::presented == false`), so this is not a downgrade — it is the
/// format that path already produces, and what egui expects of a `ColorImage`.
#[cfg(not(feature = "mind-edition"))]
const MIRROR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl FrameOutput {
    /// The frame's destination, taken from the target the caller described (#572 stage 3).
    ///
    /// This used to be `resolve_output(offscreen, window)`, reconciling an explicit offscreen
    /// request against "whatever swapchain happens to exist". With the surface out of the world
    /// there is nothing left to reconcile: the caller owns the texture either way and states its
    /// own size, format and `presented`. Kept as a named constructor rather than inlined so the
    /// contract still has one place to be tested and one place to read.
    /// Deliberately takes the three scalars rather than a `&FrameTarget`: a target holds a
    /// `&wgpu::Texture`, which cannot be constructed without a GPU, and that would have put this
    /// contract permanently out of reach of the test suite. The seam is worth more tested than
    /// tidy.
    fn of(size: (u32, u32), format: wgpu::TextureFormat, presented: bool) -> FrameOutput {
        FrameOutput { size, format, presented }
    }
}

/// The composite's output headroom for this frame (`post_params.hdr_max`):
/// `1.0` = SDR (the chosen tone-map operator), `> 1.0` = true-HDR output rolling
/// off toward that many times SDR white.
///
/// **An offscreen target is always SDR.** EDR headroom is a property of a
/// *display*, measured off the window's `CAMetalLayer` by `hdr_macos::set_edr`.
/// An offscreen texture has no layer and no display: emitting unclamped
/// extended-range radiance into it would hand its consumer (an egui pane, a
/// screenshot) values it has no way to interpret. If an embedded HDR viewport is
/// ever wanted, the caller must pass its own headroom in explicitly rather than
/// inherit the window's.
///
/// `record_max` is the recorder's mastering headroom (#430) when a take is
/// rolling — the file keeps the full highlight range even if the panel can't.
fn frame_hdr_max(
    out: &FrameOutput,
    hdr_enabled: bool,
    display_max: f32,
    record_max: Option<f32>,
) -> f32 {
    if !out.presented || !hdr_enabled {
        return 1.0;
    }
    record_max.unwrap_or(display_max)
}

/// The geometry the interface lays out at on this frame, or `None` for no interface (#593 T3).
///
/// A named predicate rather than an inline `if`, for the same reason [`frame_hdr_max`] and
/// [`take_should_stop`] are: it encodes a distinction that is easy to get wrong and impossible
/// to check from the call site, because a real [`FrameTarget`] holds a `&wgpu::Texture` and
/// cannot be built without a GPU.
///
/// **The distinction it draws is between *absent* and *nonsense*, and they are not the same:**
///
/// - **No scale factor at all → no interface.** Not "lay out at 1.0". This matters because
///   `ui_scale_factor` is now the *only* thing telling the world what scale to use — the field
///   replaced a `&winit::Window` the layer used to ask — so a `None` that quietly became `1.0`
///   would render the whole interface at half size on any Retina display, and look like a theme
///   bug rather than a missing input. `None` is what every offscreen consumer passes (the frame
///   mirror, the production recorder) and it means *draw no interface*, exactly as
///   `ui_window: None` did.
/// - **A scale that cannot be divided by → `1.0`**, via [`WindowGeometry::scale`], because
///   there is no better answer and infinity is a worse one.
///
/// `presented` gates it first: an offscreen frame is a picture of the *scene*, and an interface
/// painted into the recorder's file or the frame mirror's texture would be a defect, not a
/// feature.
fn ui_geometry(out: &FrameOutput, ui_scale_factor: Option<f32>) -> Option<WindowGeometry> {
    if !out.presented {
        return None;
    }
    // The frame's own size, not the window's: those agree on a presented frame and disagree for
    // one frame across a resize, and what egui must be told about is the image it is actually
    // drawing into.
    Some(WindowGeometry::new(out.size, ui_scale_factor?))
}

/// Whether an in-progress take ends on this frame (#582).
///
/// The whole reason this is a named function rather than an inline `||` is the bug it encodes:
/// **a frame that is not presented must never end a take.** The frame mirror (#554 T1) renders a
/// second offscreen pass through the same frame body whenever an editor is open, at its own size
/// and format; an ungated auto-stop saw that pass disagree with the take's latched dimensions and
/// killed every recording after 2–3 frames. `presented` is the first term for that reason, and
/// the tests below pin it.
///
/// `done` is the N-bar musical auto-stop; `matches` is whether the frame still agrees with what
/// the take latched (aspect / format / gamut unchanged mid-take).
fn take_should_stop(presented: bool, done: bool, matches: bool) -> bool {
    presented && (done || !matches)
}

/// The composite's wide-gamut expansion flag (`post_params.gamut`). Gated on the
/// EDR surface actually being tagged Rec.2020 (#119) — which only ever happens on
/// a presented window, so an offscreen frame never expands its gamut.
fn frame_gamut(out: &FrameOutput, hdr_enabled: bool, hdr_wide: bool) -> f32 {
    if out.presented && hdr_enabled && hdr_wide {
        1.0
    } else {
        0.0
    }
}

/// Which environment the IBL/skybox is currently built from (#100). The env is an
/// expensive bake, so it's rebuilt only when this value changes. Priority when
/// selecting each frame: `Atmosphere` (explicit toggle) > `Hdr` (a loaded file) >
/// `Procedural`. `Hdr` carries (ipc hdr_gen, local 'O'-key gen) so either bump
/// re-loads; `Atmosphere` carries a signature of the params + quantized sun dir so
/// a running day cycle re-bakes only every ~degree of sun motion.
#[derive(Clone, PartialEq)]
enum EnvReq {
    Procedural,
    Hdr(u32, u32),
    Atmosphere(u64),
}

/// #618 T4a — Recorder + phrase-chunk state (#430). One cluster: the encode session, its
/// pending key-latched toggles, and the musical grid the chunk mode rolls files on.
struct RecordState {
    // In-app recorder (#430 Tier 0): the active encode session (None = idle), a pending
    // toggle set by the 'R' key and consumed in render() (where the output size / format
    // / HDR state are known), the record length in bars (0 = free-run), and the beat
    // position captured when recording began (for the N-bar auto-stop off `beat_pos`).
    recorder: Option<recorder::Recorder>,
    toggle_pending: bool,
    bars: u32,
    start_beat: f64,
    // #430 perfect capture: `perfect_pending` is latched by Shift+R (chosen at the
    // key press), consumed when the recorder starts; `fixed` is the active take's
    // mode — while true, render() drives the animation at a fixed 1/FPS step (deterministic,
    // video-only) instead of wall-clock dt. The Shift modifier it latches from is
    // `World::mods_shift` — keyboard state, not recorder state, so it stays on `World`.
    perfect_pending: bool,
    fixed: bool,
    // #430 chunk mode: the encoded file rate, cycled by 'V'. 60 by default (the historic
    // behaviour); the cinematic rates exist so clips drop into an NLE sequence natively
    // instead of being conformed from 60 (which duplicates frames unevenly).
    fps: recorder::Fps,
    // #430 **phrase chunk mode** ('C'): record continuously, rolling to a new file on every
    // musical phrase boundary, so a whole take comes out as grid-aligned clips ready to drop
    // onto a music-video timeline.
    //
    // `chunk_armed` is the mode itself. While armed the recorder is spawned *warm but gated*,
    // and the boundary opens its shutter — so clip 1 starts on a downbeat, not wherever 'C'
    // was pressed.
    //
    // The grid lives in `beat_pos` space (continuous and monotonic — unlike the host's
    // `pos_beats`, which wraps mod 1024 and jumps on a locate), phase-aligned to the host at
    // arm time: clip `k` spans `[offset + k·phrase, offset + (k+1)·phrase)`. Computing each
    // anchor absolutely from `chunk_index` rather than accumulating is the same discipline
    // the frame quotas use, for the same reason.
    //
    // `chunk_bpm` is frozen at arm time so every clip in a session is quantised against one
    // tempo, and `chunk_session` is the filename stem they share.
    chunk_armed: bool,
    /// Latched by 'C', consumed in render() — arming needs the live snapshot (host phase,
    /// tempo, meter), which only render() holds.
    chunk_arm_pending: bool,
    chunk_phrase_beats: f64,
    chunk_grid_offset: f64,
    chunk_bpm: f64,
    chunk_index: u64,
    chunk_session: Option<String>,
    /// Absolute bar number (1-based, from the host) of the current clip's boundary, when the
    /// host is handing us a live `pos_beats` — it goes in the filename so a clip's place on
    /// the timeline is legible without opening it.
    chunk_bar: Option<u64>,
    // #430: takes whose blocking tail (ffmpeg wait + audio mux) is running on a background
    // thread. Joined at exit so the process can't die mid-mux and strand a `.videotmp`.
    pending_finalizers: Vec<std::thread::JoinHandle<()>>,
    // On-screen recorder feedback (#430 Tier 0). The plugin spawns this process with stderr
    // lost (see the panic-log hook in `main`), so a failed start — a missing ffmpeg, say —
    // would otherwise be completely invisible: no file, no message. `hud` is the live
    // "● REC …" line while encoding; `error` is the last start failure + when it
    // happened, so it can linger on screen for a few seconds and then clear itself.
    hud: Option<String>,
    error: Option<(String, std::time::Instant)>,
    // #430: transient toast shown when the record length is cycled with 'B' (so the armed
    // length — Free / 8 / 16 / 32 / 64 bars — is discoverable before pressing R).
    note: Option<(String, std::time::Instant)>,
}

/// #618 T4a — Plexus surface scratch (#8). Node cloud, impostor caps, morph meshes and the
/// overlay shell — all rebuilt per frame and all kept to reuse their allocations.
struct PlexusScratch {
    // Plexus surface mode (#8): scratch node cloud extracted from `instances` before
    // it's rebuilt as the proximity web (kept as fields to reuse the allocations).
    nodes: Vec<Vec3>,
    ntints: Vec<Vec4>,
    // Tier 2 impostor instance lists (node spheres as A≈B capsules, edge tubes).
    node_caps: Vec<render::MembraneArmInstance>,
    edge_caps: Vec<render::MembraneArmInstance>,
    activations: Vec<f32>, // Tier 3: per-node signal activation this frame
    // Tier-1 shape morph: the two procedurally-morphed meshes (node cube→sphere,
    // strut square→circle), rebuilt only when the shape params change; and the
    // marker/strut split for the two-batch draw (None = not Tier-1 plexus).
    node_mesh: math::TubeMesh,
    edge_mesh: math::TubeMesh,
    shape_cache: (f32, f32), // last (node_shape, edge_shape) the meshes were built for
    batches: Option<render::PlexusBatches>,
    // Plexus OVERLAY: the web wrapped as an outer shell around ANOTHER surface.
    // `sample_*` = the stride-sampled base node cloud; `ov_nodes/ov_tints` = its outer
    // shell (scaled out). Tier-1 overlay fills `ov_insts/ov_itints` (markers+struts on
    // their own instance buffers so they layer over the base surface); Tier-2/3 overlay
    // reuses `node_caps`/`edge_caps`. Impostor path shares the standalone
    // caps; Tier-1 shares the standalone morph meshes above.
    // Temporally-smoothed per-sample "shell-ness" (EMA of `math::shell_scores`), keyed
    // on the stable sample index — thresholded to give a STABLE overlay-shell membership
    // (kills the per-frame topology churn under animation).
    ov_shellness: Vec<f32>,
    ov_sample_nodes: Vec<Vec3>,
    ov_sample_tints: Vec<Vec4>,
    ov_nodes: Vec<Vec3>,
    ov_tints: Vec<Vec4>,
    ov_insts: Vec<Mat4>,
    ov_itints: Vec<Vec4>,
    overlay_batches: Option<render::PlexusBatches>,
}

/// #618 T4a — Field Chamber decor (#346). Rebuilt each frame from
/// `Shared.chamber`/`scopewave`/`audiospectrum`.
struct ChamberDecor {
    // Field Chamber (#346): rebuilt each frame from Shared.chamber/scopewave/audiospectrum.
    surfs: Vec<chamber::ChVertex>,
    lines: Vec<chamber::ChLine>,
    beads: Vec<chamber::ChBead>, // Tier 2 impostor capsules
    cam_right: [f32; 3],
    cam_up: [f32; 3],
    material: [f32; 4], // mat_type, metallic, roughness, ior
    opacity: f32,
}

/// #618 T4a — Particle Aura (#81) scratch. The CPU velocity grid rebuilt each frame, its
/// GPU upload buffer, and the node samples/velocities the mote respawn reads.
struct ParticleAura {
    // Particle Aura (#81): a reusable CPU velocity grid (rebuilt each frame from
    // the active generator — analytic where available, splatted from node motion
    // otherwise), its GPU upload buffer, the respawn-anchor node samples,
    // last-frame node positions (the splat finite-difference source), a scratch
    // velocity buffer, a per-frame RNG seed, and the (generator, count) key that
    // forces a mote re-seed when it changes.
    vel_grid: math::VelGrid,
    vel_upload: Vec<Vec4>,
    node_samples: Vec<Vec4>,
    prev_node_pos: Vec<Vec3>,
    node_vels: Vec<Vec3>,
    seed: u32,
    key: (u32, u32),
}

/// #618 T4a — Fluid Ink (#182 T1) + MLS-MPM liquid (#182 T3a) + the #247 ember glow: three
/// CPU grids over the tank and their GPU upload buffers. All mirror the vel grid.
struct FluidGrids {
    // Fluid Ink (#182 Tier 1): the CPU dye-injection grid (node colours splatted
    // into a ball around each node) + its GPU upload buffer. Mirrors the vel grid.
    dye: math::VelGrid,
    dye_upload: Vec<Vec4>,
    // MLS-MPM liquid (#182 Tier 3a): the collider-occupancy grid over the tank
    // (a VelGrid for its world→cell mapping) + its GPU upload buffer.
    occ: math::VelGrid,
    occ_upload: Vec<Vec4>,
    // #247 Tier 3 (energy → liquid): a FIELD_RES ember-glow grid over the tank that
    // energized Maxwell nodes splat into; uploaded to the liquid solver's resolve.
    glow: math::VelGrid,
    glow_upload: Vec<Vec4>,
}

/// #618 T4a — AI Performer (#317) link. The override lane shared with the worker thread, the
/// live state it answers tool calls from, and the edge-detect counters for `Shared.agent[8]`.
struct PerformerLink {
    // AI Performer (#317 Tier 1): the agent override lane (shared with the worker
    // thread that talks to the localhost model), the worker's user-message channel, the
    // agent's last reply (for the status readout), and the edge-detect counters for the
    // plugin-published `Shared.agent[8]` runtime block (chat / plan / release).
    // organon#49 T4c-i — the prompt-side param vocabulary, handed in at construction by
    // whoever built the World. It is `agent::core_catalog()` in every real caller; the
    // indirection exists because that function reads `param_table` and so lives above
    // this file, while this file is on its way down to `organon-world`.
    catalog: Vec<agent::CatSlot>,
    lane: std::sync::Arc<std::sync::Mutex<agent::AgentLane>>,
    reply: std::sync::Arc<std::sync::Mutex<String>>,
    // Live state/feedback snapshot the worker injects as read_state / read_feedback
    // tool results (finding #5). Stamped each frame from `Shared` + perf metrics.
    state: std::sync::Arc<std::sync::Mutex<agent::LiveState>>,
    tx: Option<std::sync::mpsc::Sender<String>>,
    last_chat_gen: u32,
    last_plan_gen: u32,
    last_release_gen: u32,
    // #425: last-seen name_gen (Shared.agent[4]); a bump = a preset was saved with
    // auto-naming on, so read the request sidecar and name it off-thread.
    last_name_gen: u32,
    // Append-and-drain chat cursor (finding #3): how many lines of the chat sidecar have
    // already been enqueued to the worker (so rapid sends aren't dropped, none replayed).
    chat_lines_consumed: usize,
    // Seed the edge-detect baselines from the FIRST Shared read (finding #6) so a visual
    // restart against a non-zero plugin counter doesn't fire a false edge / replay chat.
    baseline_seeded: bool,
    // Last GPU frame time (ms), mirrored from the feedback path for read_feedback.
    gpu_ms: f32,
    status_written: String,
}

impl PerformerLink {
    /// Spawn the agent worker thread once (on the first chat message). It owns the
    /// OpenAI-compatible localhost client + the conversation; it receives user messages
    /// on a channel, calls the model, parses tool calls, and dispatches them into the
    /// shared override lane. Real network I/O happens here (the Mac step) — never in a
    /// headless test (which drives `agent::dispatch` / the lane directly).
    fn ensure_agent_worker(&mut self) {
        if self.tx.is_some() {
            return;
        }
        // 🚨 organon#49 T5b — AN EMPTY CATALOG REFUSES, LOUDLY, INSTEAD OF SERVING A GUTTED
        // PROMPT. `organon-visual`'s manifest names this as the failure with no error attached
        // to it: hand `World::new` an empty catalog and everything still compiles and runs, the
        // model just silently loses every parameter it is allowed to touch. That is precisely
        // the shape of bug a host that does not run the Performer at all — Organon Console —
        // would introduce by passing `Vec::new()` and moving on.
        //
        // So the absence is made explicit here rather than trusted to a comment at each call
        // site. A host with no catalog does not get a crippled Performer; it gets none, and the
        // log says why. ⚠️ Inert for every host that passes a real catalog (the visual, the
        // standalone, the plugin) — `core_catalog()` is never empty, so this never fires there.
        if self.catalog.is_empty() {
            mind_log::append(
                mind_log::MindEvent::Brief,
                "agent",
                "no worker: this host built its World with an empty catalog, so the Performer \
                 has no vocabulary to actuate. Refusing rather than prompting the model with \
                 nothing (organon#49 T5b).",
            );
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let lane = self.lane.clone();
        let reply = self.reply.clone();
        let state = self.state.clone();
        // organon#49 T4c-i — the catalog is INJECTED, not reached for. It is built from
        // `param_table::pack_*::catalog`, which is the plugin's own automation surface and
        // cannot descend, so `agent::core_catalog` stayed in the root crate and `World::new`
        // takes the result. That is what leaves this file with no upward edge for T4c-ii,
        // which moves it into `organon-world`.
        let catalog = self.catalog.clone();
        std::thread::spawn(move || {
            let client = agent::HttpChatClient;
            let mut convo = vec![agent::ChatMessage::system(agent::system_prompt(&catalog))];
            mind_log::append(mind_log::MindEvent::Brief, "agent", agent::architecture_brief());
            for msg in rx {
                convo.push(agent::ChatMessage::user(msg));
                let cfg = agent::AgentConfig::load();
                match client.complete(&cfg, &convo, None) {
                    Ok(body) => {
                        let parsed = agent::parse_reply(&body);
                        if !parsed.text.is_empty() {
                            mind_log::append(mind_log::MindEvent::Reply, "agent", &parsed.text);
                            if let Ok(mut r) = reply.lock() {
                                *r = parsed.text.clone();
                            }
                        }
                        if parsed.tool_calls.is_empty() {
                            // Plain-text reply: record the assistant turn as before.
                            if !parsed.text.is_empty() {
                                convo.push(agent::ChatMessage::assistant(parsed.text.clone()));
                            }
                        } else {
                            // Tool reply (finding #4): push the assistant turn CARRYING the
                            // tool_calls, then a `tool`-role result per call keyed by id,
                            // BEFORE the next model call — so multi-turn tool use round-trips.
                            let wire_calls: Vec<agent::ToolCall> = parsed
                                .tool_calls
                                .iter()
                                .map(|t| agent::ToolCall {
                                    id: t.id.clone(),
                                    kind: "function".into(),
                                    function: agent::ToolFunction {
                                        name: t.name.clone(),
                                        arguments: t.arguments.clone(),
                                    },
                                })
                                .collect();
                            convo.push(agent::ChatMessage::assistant_tools(
                                parsed.text.clone(),
                                wire_calls,
                            ));
                            for inv in &parsed.tool_calls {
                                // Dispatch the action (mutates the lane, logs) and build the
                                // tool-result content the next model call will see.
                                let result = match &inv.action {
                                    Some(action) => {
                                        let mut summaries = Vec::new();
                                        if let Ok(mut lane) = lane.lock() {
                                            // #317 UI-sync → editor sliders, INSIDE the lock so the
                                            // apply channel and the lane can't diverge on a lock miss.
                                            append_agent_apply(action);
                                            for out in agent::dispatch(&mut lane, action.clone()) {
                                                let ev = if out.is_applied() {
                                                    mind_log::MindEvent::Action
                                                } else {
                                                    mind_log::MindEvent::Reject
                                                };
                                                mind_log::append(ev, "agent", out.summary());
                                                summaries.push(out.summary().to_string());
                                            }
                                        }
                                        // Inject live data for the read_* tools (finding #5).
                                        match action {
                                            agent::AgentAction::ReadState => state
                                                .lock()
                                                .map(|st| st.state_json())
                                                .unwrap_or_else(|_| "{}".into()),
                                            agent::AgentAction::ReadFeedback => state
                                                .lock()
                                                .map(|st| st.feedback_json())
                                                .unwrap_or_else(|_| "{}".into()),
                                            _ => summaries.join("; "),
                                        }
                                    }
                                    None => format!("unknown tool '{}'", inv.name),
                                };
                                convo.push(agent::ChatMessage::tool(inv.id.clone(), result));
                            }
                        }
                    }
                    Err(e) => {
                        mind_log::append(
                            mind_log::MindEvent::Note,
                            "agent",
                            &format!("client error: {e}"),
                        );
                        if let Ok(mut r) = reply.lock() {
                            *r = format!("(model error: {e})");
                        }
                    }
                }
            }
        });
        self.tx = Some(tx);
    }
}

/// #618 T4a — The external command channels (#452): the `organon` CLI lane and the Tier-3
/// "eyes" snap/record lane, both drained by file-length cursor with no Shared gen counter.
struct CmdChannel {
    // #452 Tier 2: the CLI command channel (`organon` / external agents). Growth is
    // self-detected by file length (the CLI is never an IPC writer, so there is no
    // Shared gen counter). Seeded at CONSTRUCTION (`agent::cli_seed`) — not at the
    // first drain — so a command appended right after launch still drains; only
    // the backlog from before this process existed is skipped.
    cli_cursor: usize,
    cli_len: u64,
    // #452 Tier 3 ("the eyes"): the snap/record request channel (`eyes_cmd_path`), drained
    // with the SAME file-length/cursor discipline + construction-time seed as the CLI
    // command channel above. Replies (path or error) go back on `eyes_reply_path`.
    eyes_cursor: usize,
    eyes_len: u64,
    // A `snap` request awaiting its frame: (nonce, absolute output path). Set when the
    // request drains; consumed in render() once the production texture exists (1-frame
    // latency), which reads it back to a PNG and appends the reply.
    snap_pending: Option<(String, std::path::PathBuf)>,
    // A `record start/stop` request awaiting its outcome: (nonce, is_start). The recorder
    // path is only known after `Recorder::start()`/before `finish()` (both in the record
    // handler), so the reply is appended there, not at drain time.
    eyes_record_pending: Option<(String, bool)>,
}

/// #618 T4a — #648 T1 — the strand bundle and everything lowered from it: the raster
/// instances/tints, the RT cylinder set, the welded-mode node anchors, and the welded mesh.
/// `emit_strands` lives on THIS type, which is what takes it off `&mut World` and lets
/// `frame_body` hold other clusters across its 29 call sites.
struct Geometry {
    instances: Vec<Mat4>,
    tints: Vec<Vec4>, // per-instance colour tint (Swept-Tubes HSV sweep; else white)
    // organon#217 T1 — per-instance EMISSION, parallel to `instances` when the glyph
    // ring drives the frame and EMPTY otherwise. Cleared every frame beside
    // `rt_instances`; only `glyph_grid_geometry` fills it. The renderer treats any
    // length other than `instances.len()` as "no emission" (all zero), so a generator
    // that never heard of this field is byte-identical to before it existed.
    emits: Vec<Vec4>,
    // RT / path-tracer geometry for Contiguous Swept Tubes: the raster draws the
    // welded `swept_mesh` with `instances` empty, so the ray tracer has nothing to
    // trace. These carry the per-segment cylinder instances (the same `lower_strands`
    // output the non-welded mode uses) so the TLAS + PT hit-shading work in welded
    // mode. Empty in every non-welded path (RT then uses `instances`).
    rt_instances: Vec<Mat4>,
    rt_tints: Vec<Vec4>,
    // True only when welded AND some RT feature (path tracer / shadows / reflections /
    // AO / GI) is actually live — gates the extra per-frame cylinder lowering + buffer
    // upload so plain welded views (no RT) pay nothing.
    rt_geo_wanted: bool,
    // Node-driven systems (particle aura / fluid ink / liquid colliders) source their
    // node anchors from `instances`, which Contiguous Swept Tubes clears (the raster
    // draws the welded `swept_mesh`). Like `rt_instances`, this keeps the per-segment
    // cylinder lowering so those systems still have node positions in welded mode —
    // exactly what non-welded Swept Tubes already feeds them. Refilled by `emit_strands`
    // only when a node-driven system is live (`need_weld_nodes`); empty otherwise.
    node_insts_weld: Vec<Mat4>,
    node_tints_weld: Vec<Vec4>,
    // Set each frame before geometry building: is any node-driven system on? Gates the
    // welded node-anchor lowering above.
    need_weld_nodes: bool,
    // Strand-emitting generators (e.g. Frenet) build their bundle here, then
    // `math::lower_strands` turns it into the instances/tints above. Reused.
    gen_strands: math::Strands,
    // Contiguous Swept-Tubes: the welded mesh, rebuilt each frame when the mode is
    // Swept Tubes + Contiguous. Empty otherwise (the instanced path is used).
    swept_mesh: math::TubeMesh,
    // Welded-tube cross-section shape (1 = circle … 0 = sharp square), stashed each
    // frame so the many `emit_strands` sites don't each need to thread it.
    tube_profile: f32,
}

/// #648 T1 — `emit_strands` lives here, not on `World`. That is the whole point of the
/// cluster: it is called 29 times from `frame_body`, and while it took `&mut self` no
/// other cluster could be held as a `&mut` local across any of those calls. Its body is
/// byte-for-byte what it was on `World` — the field names inside `Geometry` are unchanged
/// precisely so that stayed true, and `check-world-partition.py` check D asserts it.
impl Geometry {
    /// Membrane Skin-Arms Impostor build: lower `gen_strands` to one capsule
    /// impostor per segment (endpoint A + radius, endpoint B, strand tint). Fills
    /// `caps` and returns the node bounds. Leaves `instances` / `swept_mesh`
    /// for the caller to clear (nothing else draws this geometry).
    ///
    /// `radius_override` > 0 forces a uniform capsule radius (else the per-node
    /// thickness). Consecutive capsules are **overlapped** — each is extended past
    /// its shared node by half a radius along the segment — so the segment bodies
    /// interpenetrate and the sphere-trace can't leave a crack between them at
    /// grazing/side angles (the seamless-tube look without a mesh).
    /// `caps` is membrane-impostor OUTPUT — read by the renderer, cleared elsewhere —
    /// not strand geometry, so it is a parameter rather than a `Geometry` field. At the
    /// call site it is a disjoint borrow of `self`.
    fn build_arm_caps(
        &mut self,
        radius_override: f32,
        caps: &mut Vec<render::MembraneArmInstance>,
    ) -> math::Bounds {
        caps.clear();
        let mut bounds = math::Bounds::new();
        for strand in &self.gen_strands {
            if strand.len() < 2 {
                continue;
            }
            for w in strand.windows(2) {
                let (a, b) = (w[0].position, w[1].position);
                let r = if radius_override > 0.0 {
                    radius_override
                } else {
                    w[0].scale.x.max(0.02)
                };
                // Overlap: push each end outward along the segment so neighbours
                // interpenetrate (clamped to the segment length so short segments
                // don't invert).
                let seg = b - a;
                let len = seg.length();
                let dir = if len > 1e-6 { seg / len } else { Vec3::ZERO };
                let ov = (r * 0.5).min(len * 0.5);
                let (ae, be) = (a - dir * ov, b + dir * ov);
                let t = w[0].tint;
                bounds.min = bounds.min.min(a).min(b);
                bounds.max = bounds.max.max(a).max(b);
                caps.push(render::MembraneArmInstance {
                    a_r: [ae.x, ae.y, ae.z, r],
                    b: [be.x, be.y, be.z, 0.0],
                    color: [t.x, t.y, t.z, 1.0],
                });
            }
        }
        bounds
    }

    /// Lower the shared `gen_strands` to renderable geometry: normally the
    /// per-segment instanced rods (`lower_strands`), but when Contiguous Swept-Tubes
    /// is on (`weld`) instead weld each strand into one continuous tube
    /// (`weld_strands`) and clear the instances so the instanced draw + SSAO prepass
    /// skip. Used by every Streamlines generator at its lowering site.
    fn emit_strands(&mut self, fa: bool, caps: math::CapParams, weld: bool) -> math::Bounds {
        if weld {
            self.instances.clear();
            self.tints.clear();
            // The raster draws the welded mesh, but the ray tracer needs geometry:
            // lower the SAME strands to per-segment cylinders for the TLAS + PT hit
            // shading — only when an RT feature is live (`rt_geo_wanted`).
            if self.rt_geo_wanted {
                math::lower_strands(&self.gen_strands, fa, &mut self.rt_instances, &mut self.rt_tints);
            }
            // Same idea for the node-driven systems (aura / ink / liquid colliders):
            // they need node anchors that `instances` no longer carries in welded mode.
            // Lower the strands to the per-segment rods (identical to what non-welded
            // Swept Tubes feeds them) so the aura/ink don't die when Contiguous is on.
            if self.need_weld_nodes {
                math::lower_strands(
                    &self.gen_strands, fa, &mut self.node_insts_weld, &mut self.node_tints_weld,
                );
            }
            math::weld_strands(&self.gen_strands, caps, self.tube_profile, &mut self.swept_mesh)
        } else {
            // Non-welded: lower the shared strands to per-segment instanced rods.
            // (Was `World::emit_strands(fa, caps, weld)` calling itself — infinite
            // recursion; PR #276. Named for the old receiver: this method lived on
            // `World` until #648 T1 moved it here.)
            math::lower_strands(&self.gen_strands, fa, &mut self.instances, &mut self.tints)
        }
    }
}

/// organon#217 T1 — how long the world keeps drawing the last grid after the producer
/// stops publishing, in seconds. A live producer never goes quiet this long (it
/// heartbeats every 250 ms through the dwell), so this is only ever crossed by a
/// producer that has exited — or by yesterday's ring file, still in `$TMPDIR`, which
/// would otherwise draw a frozen grid at every launch forever.
const GLYPH_SILENCE_S: f32 = 3.0;

/// organon#217 T5 — what this frame's glyph ring contributes to the path tracer.
/// Captured once per frame where `glyph_grid_geometry` decides whether the ring is
/// drawing, and read where the tracer's gate and its accumulation restart are decided
/// (`pathtrace_active`, `pt_content_key`). `Default` is "no ring": `live == false`,
/// `generation == 0`, `settled == false` — a constant, so a session with no ring
/// produces exactly the key and the gate it produced before T5 (invariant #4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct GlyphPtState {
    /// A glyph frame is drawing THIS frame: ring open, layout accepted, written at
    /// least once, and not silent (`GLYPH_SILENCE_S`). ⚠️ Silence is not settle: a
    /// producer that exited leaves its last frame on the ring with `FRAME_SETTLED`
    /// possibly still set, and `glyph_grid_geometry` hands that frame back to the
    /// generator after 3 s — `live` goes false with it, so a stale grid is never
    /// path-traced as though it were held.
    live: bool,
    /// `GlyphFrame.generation` — bumps only when the cell PAYLOAD changes (the writer
    /// compares against its last publish), so the dwell's 250 ms heartbeat republish
    /// keeps it. Keying accumulation on `seq` or `tick` instead would restart it every
    /// heartbeat and it would never converge. `0` when not live.
    generation: u32,
    /// `FRAME_SETTLED`: the effect returned `None` and this grid is the held text.
    settled: bool,
}

/// The path-trace "content key" (#258 T5 / #256 T0 / organon#217 T5): every setting that
/// changes what the accumulation buffer HOLDS, plus the glyph ring's `(live, generation)`.
/// A change between frames restarts the progressive sample count; equality accumulates.
/// Spelled once so the field, its reset value and the key builder cannot disagree.
/// ⚠️ A struct, not one flat tuple: Rust derives `PartialEq` / `Debug` for tuples of at
/// most 12 elements, and the tracer's own settings already fill 11.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct PtContent {
    /// The tracer's own settings (#258 T2/T4/T5, #256 T0), exactly the pre-T5 tuple.
    tracer: (bool, u32, bool, bool, u32, u32, bool, u32, u32, u32, u32),
    /// The glyph ring's `(live, generation)` (organon#217 T5).
    glyph: (bool, u32),
}

/// `pt_prev_content` before any frame: no dielectric, Replace composite, no spectral, no
/// caustics, no cache, no ring.
const PT_CONTENT_NONE: PtContent = PtContent {
    tracer: (false, 0, false, false, 0, 0, false, 0, 0, 0, 0),
    glyph: (false, 0),
};

/// Pure: the content key for a frame. `s` supplies the tracer's own settings; `glyph`
/// supplies the ring's `(live, generation)` tail.
///
/// ⚠️ **Why a geometry counter belongs here when geometry in general does not.** The
/// tracer deliberately does NOT restart on geometry change — a moving field would smear
/// the average, and the TLAS rebuilds every frame, so "the geometry changed" is true of
/// nearly every frame and keying on it would mean never accumulating. The glyph ring's
/// `generation` is different in kind: it is a counter the *producer* bumps only when the
/// cell payload differs from its last publish, and holds through the dwell's heartbeat.
/// So it restarts accumulation exactly when the glyphs moved and accumulates exactly
/// while they are held — which is what makes restarting on it safe here and unsafe as a
/// general rule. The `live` bit beside it means "no ring" and "ring at generation 0"
/// cannot collide, and that ring → silence → generator is itself a content change.
fn pt_content_key(s: &ipc::Shared, glyph: GlyphPtState) -> PtContent {
    PtContent {
        tracer: (
            s.ptglass[0] > 0.5,
            s.ptglass[2].round().clamp(0.0, 2.0) as u32,
            s.spectral[0] > 0.5,
            s.ptcaustic[0] > 0.5,
            s.ptcaustic[2].to_bits(),
            s.ptcaustic[3].to_bits(),
            // #256 T0: the radiance cache changes what the tracer's early-terminated
            // paths hold — toggling it, or changing the query's confidence / terminate
            // bounce, or the cache identity (seed / frequency, which rebuilds the
            // network), must restart accumulation so cache and full-trace samples
            // don't blend into a frozen after-image.
            s.nrc[0] > 0.5,
            s.nrc[1].to_bits(),
            s.nrc[4].round().clamp(0.0, 8.0) as u32,
            (s.nrc[6].max(1.0)) as u32,
            s.nrc[3].clamp(0.1, 32.0).to_bits(),
        ),
        // organon#217 T5: the glyph ring — see the doc comment above.
        glyph: (glyph.live, glyph.generation),
    }
}

/// Pure: does the path tracer run this frame? **The raster → path-trace handover
/// (organon#217 T5, `doc/pbr_text_engine.md` §8).** The exact condition:
///
/// > the preset's own toggle (`pathtrace_on` — the editor checkbox or the 'P' key),
/// > OR a glyph frame is drawing this frame AND it carries `FRAME_SETTLED`.
///
/// So a preset that already path-traces is untouched (the OR is already true, and the
/// ring's phase changes nothing), a preset that rasters rasters through every frame of
/// an effect's motion and hands the frame to the tracer for the dwell, and a session
/// with no ring reduces to `preset_pt` alone — byte-identical to before T5. The tracer's
/// other gates (`hide_generator`, a boids creature, the render path being `Instanced`
/// with instances, ray-query support) still apply on top, unchanged: this decides only
/// what `pathtrace_on` used to decide alone.
fn pathtrace_active(preset_pt: bool, glyph: GlyphPtState) -> bool {
    preset_pt || (glyph.live && glyph.settled)
}

/// Pure: does the progressive sample count restart this frame? `active` is
/// [`pathtrace_active`]'s answer, not the preset toggle — while the tracer is off for
/// any reason (including the glyph ring's motion phase) the count is held at 0, so the
/// dwell's first traced frame starts from a clean buffer, never from the previous
/// effect's hold.
fn pathtrace_restarts(moved: bool, resized: bool, content_changed: bool, active: bool) -> bool {
    moved || resized || content_changed || !active
}

/// organon#217 T1 — the glyph-ring consumer. Lives on `World` (not `Geometry`)
/// because it owns the reader and the two grids beside `mind_ring`, and is called
/// once per frame from the geometry build.
impl World {
    /// If the glyph ring is live, lower its latest grid into `geom.instances` /
    /// `geom.tints` / `geom.emits` (replacing the generator's) and return the tiles'
    /// bounds. `None` — the ordinary case — leaves the frame exactly as the generator
    /// built it.
    fn glyph_grid_geometry(&mut self, look: &glyph_ring::GlyphLook, opts: glyph_ring::LowerOptions) -> Option<math::Bounds> {
        // Lazily re-open, throttled: a missing ring is the normal state.
        if !self.glyph_ring.is_open() {
            let now = Instant::now();
            if now < self.glyph_reopen_at {
                return None;
            }
            self.glyph_reopen_at = now + std::time::Duration::from_millis(500);
            self.glyph_ring = glyph_ring::GlyphRingReader::open();
            if !self.glyph_ring.is_open() {
                return None;
            }
        }
        // `seq()` is `None` for a ring the reader refuses (wrong layout) and `0` for
        // one nobody has written yet; both mean "nothing to draw" and neither is an
        // error the world should shout about.
        let seq = self.glyph_ring.seq().unwrap_or(0);
        if seq == 0 {
            return None;
        }
        // One reading of the world clock for both the arrival and the build, so a
        // frame read and built in the same call sees `since == 0` exactly.
        let now_s = self.glyph_t0.elapsed().as_secs_f64();
        if seq != self.glyph_seen_seq {
            // A new frame is read into the scratch grid first, so the blend clock can
            // say what it is before anything rotates: a heartbeat (same `epoch` and
            // `tick` — the settle publish, a dwell republish, a T11 trail decaying)
            // replaces the current grid and leaves the previous one and the clock
            // alone; a tick or a cut rotates current → previous. A copy that tears on
            // every retry keeps drawing the frame we already had.
            if self.glyph_ring.latest_into(&mut self.glyph_next) {
                let cur = (self.glyph_seen_seq != 0).then_some(&self.glyph_grid.frame);
                let arrival = self.glyph_clock.arrive(cur, &self.glyph_next.frame, self.glyph_next.tick_hz, now_s);
                if arrival != glyph_ring::Arrival::Heartbeat {
                    std::mem::swap(&mut self.glyph_prev, &mut self.glyph_grid);
                }
                std::mem::swap(&mut self.glyph_grid, &mut self.glyph_next);
                self.glyph_seen_seq = seq;
                self.glyph_seen_at = Instant::now();
            }
        }
        if self.glyph_seen_seq == 0 {
            return None;
        }
        let since = self.glyph_seen_at.elapsed().as_secs_f32();
        if since > GLYPH_SILENCE_S {
            return None;
        }
        // The §7 blend: how far between its two grids this frame is, on producer time
        // (`glyph_ring::BlendClock` — the pair's `Δtick / tick_hz`, the time since the
        // pair started, and this world's own frame interval as the lead, because the
        // frame built now is shown one interval later). A new epoch is a new effect —
        // a cut by definition — and the clock answers 1 for it, so it never slides
        // from the old one; the lowering's `blend < 1` gate then ignores `prev`.
        let blend = self.glyph_clock.blend(now_s);
        self.geom.instances.clear();
        self.geom.tints.clear();
        self.geom.emits.clear();
        let prev = (self.glyph_prev.frame.seq != 0).then_some(&self.glyph_prev);
        // organon#217 T3: `look` is `glyph_look_from(&s)` — the param chain's
        // `Shared.glyph`, which on a default snapshot IS `GlyphLook::DEFAULT`. T9: `opts`
        // is `glyph_lower_options(&s.glyph)` — the dark-tile switch off `glyph[14]`, and
        // at its default `lower_grid_with` IS `lower_grid` (pinned in `glyph_ring`).
        let bounds = glyph_ring::lower_grid_with(
            &self.glyph_grid,
            prev,
            blend,
            look,
            opts,
            glyph_ring::TileOut {
                instances: &mut self.geom.instances,
                tints: &mut self.geom.tints,
                emits: &mut self.geom.emits,
            },
        );
        Some(bounds)
    }
}

/// #618 T4a — #648 T2 — the HDR display/tone state: the loaded .hdr path and its gen
/// counters, the live enable + headroom, and the wide-gamut pair. `set_hdr` lives here.
struct HdrState {
    hdr_path: Option<String>,
    last_hdr_gens: (u32, u32),
    local_hdr_gen: u32,
    // True HDR output (macOS EDR). Toggled with the 'H' key or the editor's
    // Renderer checkbox (via IPC). `hdr_max` is the display's measured EDR
    // headroom, fed to the composite tonemap (1.0 = SDR). `last_hdr_ipc` is the
    // last IPC value seen, so we apply the editor checkbox only on a change (and
    // the 'H' key can still flip state without the IPC value fighting it).
    hdr_enabled: bool,
    hdr_max: f32,
    last_hdr_ipc: bool,
    // Wide-gamut (Rec.2020) HDR colorspace: applied state + last IPC value seen.
    hdr_wide: bool,
    last_hdr_wide: bool,
}

impl HdrState {
    /// Set true HDR (macOS EDR) output on/off. Swaps the swapchain between the
    /// sRGB SDR surface and an `Rgba16Float` HDR surface, rebuilds the composite
    /// pipeline for the new format, flips the metal layer into an extended-linear
    /// colorspace, and reads the display's EDR headroom into `hdr_max`. Idempotent:
    /// a no-op if already in the requested state. Driven by the 'H' key and by the
    /// editor's Renderer checkbox (via the IPC `hdr_output` flag).
    fn set_hdr(&mut self, enable: bool) {
        if self.hdr_enabled == enable {
            return;
        }
        // (#572 stage 3) The swapchain format swap, the metal-layer colorspace and the headroom
        // measurement are the host's — it owns the surface and the layer. Two consequences worth
        // being explicit about, because both used to be done here:
        //   * the renderer's composite/FX/temporal pipelines are NOT rebuilt here. They do not
        //     need to be: the frame already rebuilds them whenever `target.format` differs from
        //     `Gfx::out_format`, which is exactly what a format swap produces on the next frame.
        //   * `hdr_max` is no longer read here. It arrives per frame on the target, so a host
        //     that cannot grant headroom simply never sends any.
        // A host with no HDR surface refuses by leaving `hdr_max` at 1.0; the flag below still
        // flips, which keeps the editor checkbox and the **H** key honest about intent.
        self.hdr_enabled = enable;
        // No log line here. This used to report `self.hdr_max`, but since stage 3 the world does
        // not measure headroom — the host does, and delivers it on the next frame's target. So
        // the number here is always one transition stale, which is exactly the `1.00×` that then
        // gets corrected a frame later. Reported on the #582 Mac pass as "EDR applied twice": it
        // is applied ONCE, and printed twice. The host owns the message.
    }
}

/// #618 T4a — #648 T2 — Field Engine (#381 T1): the compiled program, its source, and the
/// key (preset id + field_gen) that decides when a recompile is due. `ensure_field_program`
/// lives here.
struct FieldProgram {
    // Field Engine (#381 Tier 1): the compiled program + the state that keyed the
    // last compile (the gallery preset id, and the `field_gen` counter for Custom
    // sidecar reloads). Recompiled only when the preset or `field_gen` changes; the
    // live coefficients (t/a/b) are re-bound every frame without recompiling.
    field_program: Option<math::FieldProgram>,
    field_program_src: String,
    field_program_key: (u32, u32), // (preset, field_gen)
    last_field_gen: u32,
}

impl FieldProgram {
    /// Field Engine (#381 Tier 1): (re)compile the field program when the gallery
    /// preset or the sidecar `field_gen` counter changes, caching the result in
    /// `self.field_program`. A Custom preset reads the program TEXT from the
    /// `organic-math-field.txt` sidecar (edge-detected exactly like `nn_gen`); a
    /// gallery preset uses the built-in source. On a compile error the program is
    /// left `None` (the arm falls back to an empty node set) and the error logged.
    fn ensure_field_program(&mut self, s: &ipc::Shared) {
        let preset = s.field[1] as u32;
        let key = (preset, s.field_gen);
        // Gate on the key alone (not `field_program.is_some()`): a malformed Custom
        // sidecar that fails to compile leaves `field_program == None`, and re-checking
        // `is_some()` would retry file I/O + recompilation + stderr logging every frame.
        // The key is a sentinel `(u32::MAX, u32::MAX)` at startup, so the first real
        // frame still compiles once.
        if self.field_program_key == key {
            return;
        }
        self.field_program_key = key;
        self.last_field_gen = s.field_gen;
        // `==` (not `>=`): the operator presets (#381 Tier 2) sit *above* the fixed
        // Custom sentinel (8/9/10), so only code 7 means the sidecar.
        let src = if preset == math::FIELD_PRESET_CUSTOM {
            std::fs::read_to_string(ipc::field_sidecar_path()).unwrap_or_default()
        } else {
            math::field_gallery_src(preset).to_string()
        };
        // Fall back to the Coulomb gallery program if a Custom sidecar is empty.
        let src = if src.trim().is_empty() {
            math::field_gallery_src(0).to_string()
        } else {
            src
        };
        match math::FieldProgram::compile(&src) {
            Ok(prog) => {
                self.field_program = Some(prog);
                self.field_program_src = src;
            }
            Err(e) => {
                eprintln!("field: compile failed: {e} — program: {src:?}");
                self.field_program = None;
            }
        }
    }
}

/// #618 T4a — #648 T2 — the #412 T3 persistent CPU FDTD Maxwell grid and its sub-step
/// counter. `run_fdtd` lives here. NB the volume grid it bakes into is #348 Field Volume,
/// a DIFFERENT subsystem with five other writers, so it is a parameter, not a field.
struct Fdtd {
    // #412 Tier 3 Phase 0: the persistent CPU FDTD Maxwell grid (time-marched across
    // frames while `fdtd_on`), and its running sub-step counter (drives the source
    // clock). `None` when the solver is off (frees the grid).
    fdtd: Option<math::FdtdGrid>,
    fdtd_step: u64,
    // #412: the sponge thickness currently applied to `fdtd`, so a change to the
    // boundary slider re-applies `set_sponge` at runtime (not only on grid rebuild).
    fdtd_sponge: usize,
}

impl Fdtd {
    /// #412 Tier 3 Phase 0: march the persistent CPU FDTD grid one frame and fill
    /// `field_vol_grid` from its live energy density. Runs a fixed `substeps`
    /// Yee steps/frame (deterministic; wall-clock-decoupled), injecting ẑ-polarized
    /// soft sources at the Maxwell source positions (Pulse = periodic Gaussian
    /// wavelet, CW = continuous sinusoid). The grid is rebuilt on a resolution /
    /// extent change. Colour: warm = E-dominant, cool = B-dominant.
    /// `vol` is #348 Field Volume and `gen_phase` is generator animation state — neither
    /// belongs to the FDTD solver (the volume grid has five other writers), so they are
    /// parameters rather than fields. At the call site both are disjoint borrows of `self`.
    fn run_fdtd(
        &mut self,
        s: &ipc::Shared,
        min: Vec3,
        max: Vec3,
        gen_phase: f64,
        vol: &mut Vec<glam::Vec4>,
    ) {
        let f = &s.fdtd;
        let res = (f[1] as usize).clamp(16, 128);
        let extent = f[7].max(1.0);
        // (Re)build on a resolution / extent change or first use.
        let stale = self
            .fdtd
            .as_ref()
            .map_or(true, |g| g.n != res || (-g.origin.x - extent).abs() > 1.0e-3);
        if stale {
            self.fdtd = Some(math::FdtdGrid::new(res, extent, 0.5));
            self.fdtd_step = 0;
            self.fdtd_sponge = usize::MAX; // force the sponge (re)apply below
        }
        // Re-apply the sponge whenever the boundary slider changes at runtime — not
        // only on rebuild — so toggling it to 0 for a reflecting box actually clears
        // the absorbing layer. (`set_sponge` overwrites the whole σ field.)
        let sponge = (f[6] as usize).min(res / 3);
        if sponge != self.fdtd_sponge {
            self.fdtd.as_mut().unwrap().set_sponge(sponge, 6.0);
            self.fdtd_sponge = sponge;
        }
        let source_mode = organon_core::params::FdtdSource::from_u32(f[2] as u32);
        let omega = f[3].max(0.0);
        let drive = f[4].max(0.0);
        let substeps = (f[5] as usize).clamp(1, 64);
        // ẑ-polarized soft sources at the Maxwell source layout (interference for free).
        let m = &s.maxwell;
        let sources = math::maxwell_sources(
            (m[2] as usize).max(1),
            m[4],
            m[3] > 0.5,
            m[5],
            m[6],
            gen_phase as f32,
        );
        let grid = self.fdtd.as_mut().unwrap();
        let dt = grid.dt;
        let period = if omega > 1.0e-3 { std::f32::consts::TAU / omega } else { 4.0 };
        let tau = (period / 6.0).max(dt * 3.0);
        for _ in 0..substeps {
            let t = self.fdtd_step as f32 * dt;
            if drive != 0.0 {
                // Each source carries its own `phase` (from the Maxwell phase_offset /
                // multipole layout). Drive each at `phase + src.phase` so multi-source
                // dipole interference matches the closed-form path — a CW offset shifts
                // the oscillation; a pulse offset shifts its arrival time by phase/ω.
                for src in &sources {
                    let shape = match source_mode {
                        organon_core::params::FdtdSource::Cw => (omega * t + src.phase).sin(),
                        organon_core::params::FdtdSource::Pulse => {
                            let ts = if omega > 1.0e-3 { t + src.phase / omega } else { t };
                            math::fdtd_gaussian_pulse(ts % period, period * 0.5, tau)
                        }
                    };
                    let amp = drive * shape;
                    if amp != 0.0 {
                        let (ci, cj, ck) = grid.world_to_cell(src.pos);
                        grid.add_e_soft(ci, cj, ck, Vec3::new(0.0, 0.0, amp * src.q));
                    }
                }
            }
            grid.step();
            self.fdtd_step = self.fdtd_step.wrapping_add(1);
        }
        // Fill the Volume energy cloud (warm = E-dominant, cool = B-dominant).
        let colour_e = Vec3::new(1.0, 0.55, 0.2);
        let colour_b = Vec3::new(0.2, 0.55, 1.0);
        *vol = grid.fill_volume(render::FIELD_RES, min, max, colour_e, colour_b, 6.0);
    }
}

pub struct World {
    gfx: Option<Gfx>,
    reader: ipc::Reader,
    /// #554 T1 — the frame mirror's GPU + IPC resources, present whenever
    /// `Shared::mindview_mirror()` is set, i.e. whenever an editor is running and wants frames.
    /// There is no UI element behind it: the viewport is native to the editor window, so the
    /// editor asks unconditionally rather than on a toggle. `None` costs nothing: no texture, no
    /// staging buffer, no ring file, and `pump_mirror` returns immediately — which is what a
    /// projector-only session (visual open, no editor) gets.
    ///
    /// #593 Tier 4 — the three mirror fields are full Organon's only; see [`Mirror`].
    #[cfg(not(feature = "mind-edition"))]
    mirror: Option<Mirror>,
    /// Does the editor want the mirror? Latched from `Shared.mindview[3]` inside `render_into`,
    /// where the snapshot is already in hand — reading it again in `render` would undo the
    /// read-once optimisation that function documents.
    #[cfg(not(feature = "mind-edition"))]
    mirror_want: bool,
    /// Frames since the last mirror publish, for the fixed pacing described on [`Mirror`].
    #[cfg(not(feature = "mind-edition"))]
    mirror_tick: u32,
    geom: Geometry,
    plexus: PlexusScratch,
    // #380 Density-Map Attractor: reusable scratch buffer for the iterated point set
    // (avoids a per-frame allocation of the ~tens-of-thousands of Vec2 points).
    map_points: Vec<Vec2>,
    // #380 Tier 2: scratch buffer for the overlay inset's (a,b) orbit trajectory.
    map_orbit_traj: Vec<Vec2>,
    // #248 beat → swirl: the turbine's angular momentum. Each beat kicks it (one
    // direction); it coasts down between beats (mn_swirl_decay). Drives the swirl `osc`.
    swirl_spin: f64,
    // #248 coupled dynamo: the E (axial pump) and B (swirl) mode amplitudes of the struck
    // E↔B cavity — the beat kicks `em_e`, energy rings into `em_b` and back (em_cavity_step).
    em_e: f32,
    em_b: f32,
    // #248 hue cycle: each beat pulse advances this (mn_hue_cycle) so the energized motes
    // cycle through the hue wheel with the music. Wrapped to [0,1) (hue is mod 1).
    hue_phase: f64,
    // Neural Tissue (#260 Tier 1): when the Neural Network generator lowers to
    // closed anatomical primitives, this records the soma/capsule/bouton sub-batch
    // counts so the renderer can split the ONE instance buffer into three per-mesh
    // draws. None = ordinary single-mesh instancing (set fresh each frame).
    neural_batches: Option<render::NeuralBatches>,
    // Demo scene bench (#288): the per-(mesh,material) sub-batches partitioning the
    // instance buffer, plus the scene's placeable lights. Empty = not a Demo frame
    // (set fresh each frame). The renderer draws each batch with its own mesh + a
    // patched material uniform (the scenery/water group-0 patch pattern).
    demo_batches: Vec<math::DemoBatch>,
    demo_lights: Vec<math::DemoLight>,
    /// Generic generator animation phase (advanced by the global Speed each frame,
    /// incl. the speed-pulse multiplier → beat-reactive). Frenet feeds it into the
    /// κ/τ waveform so the curve winds/unwinds in time. Original ignores it.
    gen_phase: f64,
    // Strange-attractor state (stateful: the trail slides along each trajectory).
    // `attr_heads` = current integration position per seed (f64 for stability);
    // `attr_trails` = the recent raw points per seed (ring buffer, display-scaled
    // at build). `attr_key` = (field, seeds, seed_val) → reseed when it changes.
    attr_heads: Vec<DVec3>,
    attr_trails: Vec<VecDeque<Vec3>>,
    attr_key: (u32, usize, i32),
    // Boids / flocking state (#52) — the first stateful generator. `boids` carries
    // the agent sim (pos/vel + per-agent trail ring buffers) across frames;
    // `boids_accum` is the fixed-dt accumulator (so the sim is frame-rate-stable);
    // `boids_key` = (count, seed, trail) → reseed when it changes.
    boids: math::BoidsSim,
    boids_accum: f64,
    boids_key: (usize, u32, usize),
    // Neural Network signal-propagation state (#226 Tier 2). `neural_sim` carries
    // the activation cascade across frames; `neural_key` = the structural graph key
    // (topo, nodes, connectivity, layers, seed) → rebuild the sim (+ graph) when it
    // changes so node/edge indices stay consistent.
    neural_sim: math::NeuralSim,
    neural_key: (u32, usize, usize, usize, u32),
    // Ingested connectome (#226 Tier 3): the graph loaded from the JSON sidecar
    // (`neural_graph_from_json`) when `Shared.nn_gen` bumps. `neural_load_gen` is a
    // local monotonic tag folded into the sim key so a reload rebuilds the sim.
    neural_loaded: Option<math::NeuralGraph>,
    // #367 Tier 2 (live inference): the activation-ring reader. A running model (the
    // synthetic `organic-math-mind-writer` bin now, the embedded runtime later) writes
    // per-token activation frames into a SEPARATE mmap; the `topo == 5` seam overwrites
    // `node_scalar` from the latest frame so the #226 node-glow fires per token — no
    // `Shared` change, no shader change.
    //
    // ⚠️ **There is no "Live" mode, and this comment used to say `mind[2] == 1.0` selected
    // one.** Frames arriving ARE live: the glow rides the ring whenever it has frames, on
    // whichever geometry is selected, so it is a fact about the ring rather than a mode.
    // `mind[2] == 1.0` now means **Galaxy**, and the galaxy deliberately does *not* take
    // this seam (see the `mind_view_mode(...) == 0` gate at the `topo == 5` site).
    mind_ring: mind_ring::MindRingReader,
    // organon#217 T1 — the glyph-ring reader and the two grids it hands the lowering:
    // `glyph_grid` is the latest frame, `glyph_prev` the one before it (the §7 slide
    // interpolates between them, gated per cell on `SGR_ACTIVE_PATH`), `glyph_next`
    // the scratch a new frame is read into BEFORE anything rotates — so the clock can
    // say what it is first (a heartbeat replaces `glyph_grid` and leaves `glyph_prev`
    // alone; a tick or a cut rotates). `glyph_seen_seq` is the ring `seq` the current
    // grid came from (0 = none yet) and `glyph_seen_at` when it arrived — the silence
    // detector that hands the frame back to the generator once a producer has stopped,
    // and ONLY that: the blend clock is `glyph_clock` (`glyph_ring::BlendClock`, T12's
    // finding — measured from the read over a fixed tick and evaluated at build time,
    // the old clock drew a 120 Hz producer two ticks behind on a 60 Hz display and never
    // between), fed in seconds since `glyph_t0`. Like `mind_ring`, opened lazily;
    // unlike it, re-open attempts are throttled (`glyph_reopen_at`), because a missing
    // ring is this reader's NORMAL state and a stat per frame is not free.
    glyph_ring: glyph_ring::GlyphRingReader,
    glyph_grid: glyph_ring::GlyphGrid,
    glyph_prev: glyph_ring::GlyphGrid,
    glyph_next: glyph_ring::GlyphGrid,
    glyph_seen_seq: u32,
    glyph_seen_at: Instant,
    glyph_clock: glyph_ring::BlendClock,
    glyph_t0: Instant,
    glyph_reopen_at: Instant,
    // organon#217 T5 — this frame's answer to "is the ring drawing, at which
    // generation, and is it held": written beside `glyph_grid_geometry`'s call, read
    // by the path tracer's gate and its accumulation restart. `Default` = no ring.
    glyph_pt: GlyphPtState,
    // organon#217 T3 — the bounds `lower_grid` returned this frame (tiles + backplane),
    // in world units: what the held camera frames. Meaningful only while
    // `glyph_pt.live`; stale otherwise and never read then.
    glyph_bounds: math::Bounds,
    // Ingested MLP (#226 Tier 4): trained weights loaded from the same sidecar
    // (auto-detected by format); its live forward pass builds the graph each frame.
    neural_mlp: Option<math::NeuralMlp>,
    // Ingested attention tensor (#226 Tier 5): a transformer's self-attention loaded
    // from the same sidecar (auto-detected); its causal graph is built each frame.
    neural_attn_data: Option<math::NeuralAttention>,
    // Brain model (#275): the folded-hemisphere graph is expensive to build (O(n²)
    // local-cortex k-NN wiring), so it's cached and rebuilt only when the brain dials
    // change (keyed on the packed dials), not every frame. The Tier-3 parcellation
    // (named target regions) is cached alongside it.
    brain_cache: Option<(u64, math::NeuralGraph, Vec<math::BrainRegion>)>,
    // Brain model (#275 Tier 4): the last fired focal-stimulation tick, so the
    // TMS-like drive pulses at the stim rate (one injection per beat/rate tick).
    brain_stim_tick: i64,
    last_nn_gen: u32,
    // #476 Tier 2b: last-seen `creature_gen` so a creature-JSON pick re-parses once,
    // plus the loaded body plan (replaces the built-in `form` plan while present).
    last_creature_gen: u32,
    creature_loaded: Option<Vec<math::CreaturePrim>>,
    // #367 Tier 1: last-seen `mind[1]` (model_gen) so a `.gguf` pick re-parses once.
    last_model_gen: u32,
    // #507 Tier 1 (the embedding galaxy): the parsed header + path of the loaded
    // `.gguf`, kept so switching the Mind topology (`mind[2]`) between Specimen and
    // Galaxy can rebuild the graph WITHOUT re-picking the model. `None` until a model
    // loads (and cleared again on a failed load, like `neural_loaded`).
    mind_model: Option<(String, organon_core::gguf::GgufHeader)>,
    // Last-seen `mind[2]` view mode — **0 = architecture specimen, 1 = embedding galaxy,
    // 2 = the #147 T3 Delta lens** — so a change rebuilds the graph once. Starts at 0 =
    // the default, so with nothing set this seam never fires and the output is
    // byte-identical to today.
    //
    // ⚠️ This once said `0 specimen / 1 live / 2 galaxy`, which is the **retired**
    // encoding from before Live stopped being a view. Decode through
    // [`math::mind_view_mode`] and never by hand: it is the one place that knows which
    // values exist, and an unknown one must fall back to the specimen rather than
    // selecting a view that is not there.
    last_mind_view: u32,
    // #520 — the projected embedding cloud, cached so moving `extent` (or switching
    // back to Galaxy) rescales the graph from memory instead of re-reading and
    // re-projecting the .gguf, which takes seconds. Cleared with `mind_model`.
    mind_galaxy: Option<organon_core::gguf_data::GalaxyProjection>,
    // #147 Tier 3 (the Delta lens): the adapter directory last read and the per-layer
    // movement measured from it, cached on the same reasoning as `mind_galaxy` — the
    // read parses a safetensors header and streams every `lora_A`/`lora_B` pair, so an
    // extent slider move must rescale from memory rather than re-read the adapter.
    // Keyed by the path so pointing the sidecar somewhere else re-reads once.
    mind_delta: Option<(String, math::DeltaSites)>,
    // Last `neural_net[6]` (extent) the loaded mind graph was BUILT at. `extent` is
    // baked into the graph at build time, so without this a slider move did nothing to
    // an already-loaded specimen/galaxy — the bug #520 fixes.
    last_mind_extent: f32,
    // #423 Tier 1 — the atlas: last-seen `atlas[0]` (atlas_gen) so a library scan
    // rebuilds the constellation once, plus the design points + profile the scan
    // produced (the roofline inset reads them each frame).
    last_atlas_gen: u32,
    /// `true` while `neural_loaded` holds the #423 **design-space constellation**
    /// (rather than a connectome / GGUF specimen). The slot is shared and last-write
    /// wins, so this records who wrote it — the axes only mean bytes/token,
    /// operational intensity and bits/weight when the atlas is the author.
    atlas_is_loaded: bool,
    atlas_points: Vec<math::DesignPoint>,
    atlas_profile: math::HardwareProfile,
    neural_load_gen: u32,
    field_prog: FieldProgram,
    // Field Engine (#381 Tier 3): the live time-marched PDE grid + the key
    // (preset, res) it was built from (reseed on change). `field_sim_beat_prev`
    // tracks the beat clock so the sim advances by the per-frame beat delta.
    field_sim: Option<math::FieldSim>,
    field_sim_key: (u32, usize),
    field_sim_beat_prev: f64,
    // Field Playback (#407 Tier A): the baked clip cached by its load counter
    // (`fieldclip_gen`), a continuous playback phase advanced off the beat clock
    // (frame = phase.floor() % nframes), and the beat value it last stepped from.
    field_clip: Option<math::FieldClip>,
    field_clip_gen: u32,
    field_clip_phase: f64,
    field_clip_beat_prev: f64,
    // Field Engine (#407 Tier B): the live Neural CA rollout + the (nca_gen, res) key
    // it was built from (reload weights + reseed on change). `neural_ca_beat_prev`
    // tracks the beat clock so the rollout advances by the per-frame beat delta.
    neural_ca: Option<math::NeuralCA>,
    neural_ca_key: (u32, usize),
    neural_ca_beat_prev: f64,
    // Soft-body bell state (#99): the persistent XPBD bell, its fixed-dt
    // accumulator, and the (nt, np, radius, θmax) key it was built from (rebuild
    // on change). Used only when the harmonic generator's Physical mode is on.
    bell: math::BellSim,
    bell_accum: f64,
    bell_key: (usize, usize, i32, i32),
    // Bell contraction-stroke phase (in pulses; advances with the beat × stroke rate).
    bell_phase: f64,
    // Metaball mode: node centres + radius + colour, built from instances/tints
    // each frame and baked into the 3D field. Reused to avoid per-frame allocation.
    meta_nodes: Vec<render::MetaNode>,
    // Field Volume (#348): the analytic field-energy density grid (FIELD_RES³ RGBA),
    // baked CPU-side for Maxwell/Acoustic when the Field Volume source selects the
    // field bake. Uploaded straight into the Volume field texture (no node voxelize).
    // Empty when the node metaball bake is used (Legacy / smoothed-node / non-field).
    field_vol_grid: Vec<glam::Vec4>,
    // #391 Tier 1: whether the probe CSV is currently logging. Rising edge (re)writes
    // the header + truncates the file; each subsequent frame appends one row.
    instr_csv_active: bool,
    fdtd_sim: Fdtd,
    // Membrane mode: the lofted sheet mesh (parallel arrays), rebuilt each frame.
    mem_pos: Vec<Vec3>,
    mem_norm: Vec<Vec3>,
    mem_col: Vec<Vec4>,
    mem_idx: Vec<u32>,
    // Membrane Skin-Arms Impostor build: one capsule impostor per arm segment,
    // rebuilt each frame from `gen_strands` (empty unless arms + Impostor active).
    arm_caps: Vec<render::MembraneArmInstance>,
    angle: DVec3,
    /// Wave-shaped winding phase for continuous rotation: advances each frame by
    /// `speed · wind_velocity(rot_func, angle, depth)`, so the spin always moves
    /// forward but breathes/gear-shifts with the waveform. Tracks `angle` exactly
    /// at `cont_shape = 0` (constant spin). Unused in pendulum mode.
    wind_phase: DVec3,
    /// Continuous beat position the visual advances every frame (free-running,
    /// PLL-corrected toward the host when locked). Its fractional part drives the
    /// pulse; PR2 will read whole-beat crossings for the camera-momentum kicks.
    beat_pos: f64,
    /// Previous host beat position (`transport[1]`) — the PLL only phase-locks when
    /// this is actually advancing, so a host that doesn't hand `pos_beats` to the
    /// effect (stamped as a frozen 0) can't stall the beat in Host mode.
    beat_host_pos_prev: f64,
    /// Bioluminescence phase clocks (free-running, real-time): `color_phase` slides
    /// the palette sweep; `ripple_phase` advances the travelling emissive band.
    /// Both wrap in 0..1.
    color_phase: f64,
    ripple_phase: f64,
    last_frame: Instant,
    cam_center: Vec3,
    // Manual orbit offset (drag/scroll), applied *on top of* the auto path.
    yaw: f32,
    pitch: f32,
    distance: f32,
    // Console Spike Tier 1 — the substrate rig: an ABSOLUTE camera, where everything
    // above it is relative. `Some((center, yaw, pitch, distance, roll, fov_deg))`
    // overrides all six at the finalization below, exactly as the rails branch does,
    // and latches off the `cam_center` auto-follow while it is installed. Set by
    // `set_substrate_rig`; the only caller today is Organon Console's backdrop, which
    // frames a flat plane and cannot have the field's AABB dragging the centre.
    substrate_rig: Option<(Vec3, f32, f32, f32, f32, f32)>,
    // Rails mode (#187): set each frame from the active generator. While riding,
    // drag steers `rail_off` (a bore-clamped X/Y camera offset) instead of the
    // orbit; `rails_bore` mirrors the ACTIVE block's bore for the input clamp.
    rails_active: bool,
    // #187 composite fix: true only in the PURE RIDE (scenery on + generator
    // None) — the rails camera/drag apply only then; with a generator visible
    // the orbit rig stays in charge and the corridor renders view-locked.
    rails_ride: bool,
    // #206 meander-facing camera: smoothed yaw so the scenery view faces down a
    // winding Terra channel (the river rotates underneath; the object stays
    // centred). Decays to 0 off Terra.
    channel_yaw: f32,
    rails_bore: f32,
    rail_off: (f32, f32),
    // #187 Tier 3 — the quantized-transition latch: `scenery_active_blk` is the
    // geometry block the world renders with; `scenery_pending` carries a changed
    // block + the change-every boundary (beats) where it takes over.
    // `rails_was_on` detects (re)entry into scenery so the latch re-adopts live.
    // The block is the rails timing/shape slots [0..24] ++ the Terra landform
    // slots [24..40] (#206 Tier 2), latched together so a fjord→river preset
    // recall crosses on the bar; Zone leaves the terra half at its default.
    rails_was_on: bool,
    scenery_active_blk: [f32; 40],
    scenery_pending: Option<([f32; 40], f64)>,
    // Scratch strand set for the pending world beyond the boundary plane.
    gen_strands_b: math::Strands,
    // Scenery layer (#187 pivot): the concurrent scenery geometry, rendered
    // with its own material uniforms (a second instanced draw).
    scenery_strands: math::Strands,
    scenery_instances: Vec<Mat4>,
    scenery_tints: Vec<Vec4>,
    // Scenery membrane skin (#206 Tier 1): the lofted surface for
    // ScenerySurface::Skin, drawn with the scenery material.
    scenery_mem_pos: Vec<Vec3>,
    scenery_mem_norm: Vec<Vec3>,
    scenery_mem_col: Vec<Vec4>,
    scenery_mem_idx: Vec<u32>,
    // Scenery water floor (#206 Tier 3): a rippled sheet at the per-cell water
    // level, lofted as its own membrane with its own (glass) material.
    water_strands: math::Strands,
    water_mem_pos: Vec<Vec3>,
    water_mem_norm: Vec<Vec3>,
    water_mem_col: Vec<Vec4>,
    water_mem_idx: Vec<u32>,
    // Auto-orbit state: path phase (cycles, wrapped) + angular velocity (the
    // momentum the beat kicks into). `last_beat_floor` detects beat crossings.
    cam_phase: f64,
    cam_vel: f64,
    last_beat_floor: f64,
    // Camera shot sequencer (#307 Tier 1): cycles moves on bar boundaries.
    seq: SeqState,
    // Camera storyboard (#307 Tier 3): plays an authored shot playlist.
    story: StoryState,
    // Bar-clock position (beats / beats-per-bar), cached each frame for the
    // sequencer + dolly (both driven off musical bars, not per-beat transients).
    cam_bar_pos: f64,
    // Speed Pulse envelope: kicked up by the pulse, decays back, multiplies the
    // global rotation speed by 10^(speed_bounce·amount) for a power-of-10 bounce.
    speed_bounce: f64,
    // Breath envelope: same pulse, own attack/decay; scales the whole scene.
    breath_bounce: f64,
    dragging: bool,
    cursor: (f64, f64),
    fullscreen: bool,
    // #100: the last-applied environment request (HDR gen / procedural / atmosphere
    // signature). The env is rebuilt only when this changes — folds in the old
    // hdr_gen edge-detect and the atmosphere re-bake-on-sun-move. `hdr_path` caches
    // the sidecar path (re-read only when the gen pair bumps, not every frame).
    last_env_req: Option<EnvReq>,
    hdr: HdrState,
    // #472 Tier 1: last material-folder load counter seen (edge-detect → reload).
    last_material_gen: u32,
    // #472 Tier 2: whether the procedural bake was driving the material set last
    // frame (so the falling edge restores the PNG/neutral set).
    last_material_procedural: bool,
    // #472 Tier 5: wall-time of the last animated material bake (30 Hz throttle).
    last_anim_bake: f64,
    // Path tracer (#200 Tier 4): the ground-truth toggle ('P', per-display) + the
    // progressive sample count (resets on any camera move — `pt_prev_vp`).
    pathtrace_on: bool,
    pathtrace_spp: u32,
    // Edge-detect of the editor's path-tracer checkbox (`Shared.pathtrace_on`): the
    // 'P' key sets `pathtrace_on` locally, so the IPC flag is applied only on CHANGE
    // (last-touched-wins), never overwriting a key toggle every frame — like HDR.
    last_pathtrace_ipc: bool,
    pt_prev_vp: [[f32; 4]; 4],
    // The scaled render resolution the accumulation buffers were sized to last
    // frame — a change (render-scale / window resize) recreates the ping-pong
    // buffers, so the sample count must restart too (else new rays blend against
    // wrong-resolution history).
    pt_prev_size: (u32, u32),
    // PT settings that change what the accumulation buffer HOLDS (dielectric on/off,
    // composite mode — GI-add accumulates indirect-only vs the others' full radiance).
    // Changing one while the tracer runs would blend new samples against stale
    // old-mode history (a frozen "after image"), so a change restarts the count.
    // organon#217 T5 appends the glyph ring's `(live, generation)`; `pt_content_key`
    // builds it and says why a geometry counter is admissible there.
    pt_prev_content: PtContent,
    // Neural radiance cache (#256 Tier 0): the live cache the visual owns + trains
    // and uploads to the path tracer's early-termination query. `None` until first
    // enabled; rebuilt when the seed/omega change. `nrc_key` = the (seed, omega)
    // the current cache was built with (edge-detect a rebuild). `nrc_loss` /
    // `nrc_state` are the smoothed training telemetry the editor shows.
    nrc_cache: Option<organon_core::math::RadianceCache>,
    nrc_key: (u32, u32),
    nrc_loss: f32,
    nrc_state: u32,
    // Last MSAA sample count applied, for edge-detecting the IPC param.
    last_msaa: u32,
    // Capture / production frame (#135): the effective frame-guide state (the editor
    // param sets it on a change; the 'G' key toggles it) + the last IPC guide value
    // seen (so the key isn't overwritten every frame). "Lock window to output" is
    // re-enforced each frame by comparing the live window size to the output, so it
    // needs no remembered request.
    frame_guide: bool,
    last_guide_ipc: bool,
    record: RecordState,
    /// Live Shift-key state. Keyboard input, not recorder state — it sits outside
    /// [`RecordState`] on purpose, and Shift+R reads it across that seam
    /// (`record.perfect_pending = mods_shift`). Adjacency in the old flat struct is
    /// what made the two look like one thing.
    mods_shift: bool,
    // Capture overlay (#135 Phase 2): effective enable (editor param edge-detected +
    // the 'T' key toggles it), last IPC enable seen, and the string sidecar cache
    // (handle / title override) re-read when `overlay_gen` bumps (mirrors hdr_gen).
    overlay_on: bool,
    last_overlay_ipc: bool,
    overlay_handle: String,
    overlay_title: Option<String>,
    // Capture decoration (#135 P5): rebuilt-each-frame axes surface (tubes + cones) + box
    // wall lines, and a master show/hide the 'X' key toggles (on top of the editor's
    // per-element checkboxes).
    axes_solids: Vec<axes::SurfVertex>,
    box_lines: Vec<axes::LineVertex>,
    chamber: ChamberDecor,
    decor_on: bool,
    last_overlay_gen: u32,
    // Terrain backdrop: the CPU copy of the active noise tile (so the synthetic
    // fly-camera can ride above the landscape), the (noise_type, seed) key it was
    // built from (re-synthesized + re-uploaded on change), and the fly clock
    // (advanced by the fly speed, so changing speed never jumps the camera).
    terrain_noise: Vec<f32>,
    terrain_key: (u32, u32),
    terrain_time: f64,
    // Day-cycle clock (advanced by terrain day_speed); drives the sun elevation.
    terrain_day: f64,
    // Starfield sidereal clock (advanced by the sky-rotation speed); wheels the sky.
    sky_time: f64,
    // Free-running wall clock (seconds) for the star twinkle scintillation.
    wall_time: f64,
    // Previous frame's scene view-proj, for the temporal pass (#152 Tier 2) camera
    // reprojection (TAA velocity). Updated at the end of each render().
    prev_view_proj: [[f32; 4]; 4],
    // Dynamic resolution: smoothed frame time (ms), the auto-chosen render scale,
    // the feedback channel to the editor, and the last reported dims (so the window
    // title is only rewritten on change).
    frame_ms: f32,
    // Smoothed CPU ms to build + encode + submit a frame (#277 Tier 2), reported
    // to the editor's performance status bar. Distinct from `frame_ms` (wall-clock
    // between frames, which folds in vsync/present wait).
    cpu_ms: f32,
    auto_scale: f32,
    // Monotone frame counter driving the TAA jitter sequence (#174 T3).
    frame_index: u64,
    // The quantized scale actually applied (#174 T2 — see the DRS hysteresis).
    applied_scale: f32,
    // Frames since the last no-writer IPC read; gates the ~1 Hz mmap re-open.
    reader_retry: u32,
    feedback: Option<ipc::FeedbackWriter>,
    last_render_dims: (u32, u32),
    particle_aura: ParticleAura,
    // #248 Tier 3: the audio-dipole's own oscillation clock — advances like
    // gen_phase but scaled by the spectral centroid (pitch → a visible, hugely
    // scaled-down rate); synced to gen_phase whenever the audio drive is off so
    // toggling is byte-identical. Plus the recent loudness history (newest first,
    // ~1 s at 60 fps) behind the retarded-amplitude waveform shells.
    maxdip_phase: f64,
    rms_hist: Vec<f32>,
    fluid_grids: FluidGrids,
    performer: PerformerLink,
    cmd_chan: CmdChannel,
}

/// Cap on node samples fed to the particle system (respawn anchors + the splat
/// source). Stride-subsampled so coverage spans the whole structure.
const MAX_NODE_SAMPLES: usize = 8192;
/// Cap on nodes splatted into the dye-injection grid (#182 Tier 1) — the ball
/// splat is O(nodes · radius³ cells), so it gets a tighter cap.
const MAX_DYE_NODES: usize = 2048;

impl World {
    /// Build a world.
    ///
    /// `catalog` is the agent's prompt-side param vocabulary — `agent::core_catalog()` in
    /// every shipping caller. ⚠️ **It is a parameter rather than a call** because
    /// `core_catalog` reads `param_table`, the plugin's automation surface, which cannot
    /// descend below the plugin crate; this file is on its way to `organon-world` in
    /// organon#49 T4c-ii and would not be able to see it. Passing an empty vec is legal
    /// and means "no params in the system prompt" — tests do exactly that.
    pub fn new(catalog: Vec<agent::CatSlot>) -> Self {
        // #452: seed the CLI command channel from ONE read — the cursor (lines
        // to skip) and the cached byte length both derive from the same content,
        // so a command appended between two separate calls can't strand itself
        // (review finding: a split read_to_string/metadata pair could leave
        // cli_len already matching while the cursor missed the new line).
        let (cli_seed_cursor, cli_seed_len) = match std::fs::read_to_string(ipc::cli_cmd_path())
        {
            Ok(body) => (agent::cli_seed(&body), body.len() as u64),
            Err(_) => (0, 0),
        };
        // #452 Tier 3: seed the eyes channel the same way — a snap/record request issued
        // before this process existed must not fire at launch (it would write a stale
        // path / start a phantom recording).
        let (eyes_seed_cursor, eyes_seed_len) = match std::fs::read_to_string(ipc::eyes_cmd_path())
        {
            Ok(body) => (agent::cli_seed(&body), body.len() as u64),
            Err(_) => (0, 0),
        };
        World {
            gfx: None,
            reader: ipc::Reader::open(),
            // #554 T1 — off until the editor asks. Nothing is allocated and no ring file
            // exists until `pump_mirror` sees `Shared.mindview[3]` set.
            #[cfg(not(feature = "mind-edition"))]
            mirror: None,
            #[cfg(not(feature = "mind-edition"))]
            mirror_want: false,
            #[cfg(not(feature = "mind-edition"))]
            mirror_tick: 0,
            geom: Geometry {
                instances: Vec::new(),
                tints: Vec::new(),
                rt_instances: Vec::new(),
                rt_tints: Vec::new(),
                rt_geo_wanted: false,
                node_insts_weld: Vec::new(),
                node_tints_weld: Vec::new(),
                need_weld_nodes: false,
                gen_strands: math::Strands::new(),
                swept_mesh: math::TubeMesh::default(),
                tube_profile: 1.0,
                emits: Vec::new(),
            },
            plexus: PlexusScratch {
                nodes: Vec::new(),
                ntints: Vec::new(),
                node_caps: Vec::new(),
                edge_caps: Vec::new(),
                activations: Vec::new(),
                node_mesh: math::TubeMesh::default(),
                edge_mesh: math::TubeMesh::default(),
                shape_cache: (-1.0, -1.0), // force a build on first Tier-1 frame
                batches: None,
                ov_shellness: Vec::new(),
                ov_sample_nodes: Vec::new(),
                ov_sample_tints: Vec::new(),
                ov_nodes: Vec::new(),
                ov_tints: Vec::new(),
                ov_insts: Vec::new(),
                ov_itints: Vec::new(),
                overlay_batches: None,
            },
            map_points: Vec::new(),
            map_orbit_traj: Vec::new(),
            swirl_spin: 0.0,
            em_e: 0.0,
            em_b: 0.0,
            hue_phase: 0.0,
            neural_batches: None,
            demo_batches: Vec::new(),
            demo_lights: Vec::new(),
            gen_phase: 0.0,
            attr_heads: Vec::new(),
            attr_trails: Vec::new(),
            attr_key: (u32::MAX, 0, 0),
            boids: math::BoidsSim::new(0, 0, 2, 1.0),
            boids_accum: 0.0,
            boids_key: (usize::MAX, u32::MAX, 0),
            neural_sim: math::NeuralSim::new(),
            neural_key: (u32::MAX, usize::MAX, 0, 0, u32::MAX),
            neural_loaded: None,
            mind_ring: mind_ring::MindRingReader::open(),
            glyph_ring: glyph_ring::GlyphRingReader::open(),
            glyph_grid: glyph_ring::GlyphGrid::default(),
            glyph_prev: glyph_ring::GlyphGrid::default(),
            glyph_next: glyph_ring::GlyphGrid::default(),
            glyph_seen_seq: 0,
            glyph_seen_at: Instant::now(),
            glyph_clock: glyph_ring::BlendClock::default(),
            glyph_t0: Instant::now(),
            glyph_reopen_at: Instant::now(),
            glyph_pt: GlyphPtState::default(),
            glyph_bounds: math::Bounds::new(),
            neural_mlp: None,
            neural_attn_data: None,
            brain_cache: None,
            brain_stim_tick: i64::MIN,
            last_nn_gen: 0,
            last_creature_gen: 0,
            creature_loaded: None,
            last_model_gen: 0,
            mind_model: None,
            last_mind_view: 0,
            mind_galaxy: None,
            mind_delta: None,
            last_mind_extent: f32::NAN,
            last_atlas_gen: 0,
            atlas_is_loaded: false,
            atlas_points: Vec::new(),
            atlas_profile: math::HardwareProfile::default(),
            neural_load_gen: 0,
            performer: PerformerLink {
                catalog,
                lane: std::sync::Arc::new(std::sync::Mutex::new(agent::AgentLane::new())),
                reply: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
                state: std::sync::Arc::new(std::sync::Mutex::new(agent::LiveState::default())),
                tx: None,
                last_chat_gen: 0,
                last_plan_gen: 0,
                last_release_gen: 0,
                last_name_gen: 0,
                chat_lines_consumed: 0,
                baseline_seeded: false,
                gpu_ms: 0.0,
                status_written: String::new(),
            },
            cmd_chan: CmdChannel {
                // #452: adopt the CLI channel's pre-existing backlog NOW (commands
                // from before this process started never replay; ones appended
                // after this instant always drain). Cursor + length from the single
                // read above — never from split calls.
                cli_cursor: cli_seed_cursor,
                cli_len: cli_seed_len,
                eyes_cursor: eyes_seed_cursor,
                eyes_len: eyes_seed_len,
                snap_pending: None,
                eyes_record_pending: None,
            },
            field_prog: FieldProgram {
                field_program: None,
                field_program_src: String::new(),
                field_program_key: (u32::MAX, u32::MAX),
                last_field_gen: 0,
            },
            field_sim: None,
            field_sim_key: (u32::MAX, usize::MAX),
            field_sim_beat_prev: 0.0,
            field_clip: None,
            field_clip_gen: u32::MAX,
            field_clip_phase: 0.0,
            field_clip_beat_prev: 0.0,
            neural_ca: None,
            neural_ca_key: (u32::MAX, usize::MAX),
            neural_ca_beat_prev: 0.0,
            bell: math::BellSim::new(3, 3, 1.0, 1.6),
            bell_accum: 0.0,
            bell_key: (usize::MAX, usize::MAX, 0, 0),
            bell_phase: 0.0,
            meta_nodes: Vec::new(),
            field_vol_grid: Vec::new(),
            instr_csv_active: false,
            fdtd_sim: Fdtd {
                fdtd: None,
                fdtd_step: 0,
                fdtd_sponge: usize::MAX,
            },
            mem_pos: Vec::new(),
            mem_norm: Vec::new(),
            mem_col: Vec::new(),
            mem_idx: Vec::new(),
            arm_caps: Vec::new(),
            color_phase: 0.0,
            ripple_phase: 0.0,
            angle: DVec3::new(0.18, 0.14, 0.11),
            // Start the wind phase aligned with `angle` so `cont_shape = 0`
            // reproduces the old continuous spin exactly.
            wind_phase: DVec3::new(0.18, 0.14, 0.11),
            beat_pos: 0.0,
            beat_host_pos_prev: 0.0,
            last_frame: Instant::now(),
            cam_center: Vec3::ZERO,
            // Named rather than spelled, so `organon console camera --reset` returns the
            // viewpoint to *the framing the window opened with* by construction instead of by
            // three numbers someone copied — see `scene_input::DEFAULT_YAW`.
            yaw: scene_input::DEFAULT_YAW,
            pitch: scene_input::DEFAULT_PITCH,
            distance: scene_input::DEFAULT_DISTANCE,
            substrate_rig: None,
            rails_active: false,
            rails_ride: false,
            channel_yaw: 0.0,
            rails_bore: 6.0,
            rail_off: (0.0, 0.0),
            rails_was_on: false,
            scenery_active_blk: [0.0; 40],
            scenery_pending: None,
            gen_strands_b: math::Strands::new(),
            scenery_strands: math::Strands::new(),
            scenery_instances: Vec::new(),
            scenery_tints: Vec::new(),
            scenery_mem_pos: Vec::new(),
            scenery_mem_norm: Vec::new(),
            scenery_mem_col: Vec::new(),
            scenery_mem_idx: Vec::new(),
            water_strands: math::Strands::new(),
            water_mem_pos: Vec::new(),
            water_mem_norm: Vec::new(),
            water_mem_col: Vec::new(),
            water_mem_idx: Vec::new(),
            cam_phase: 0.0,
            cam_vel: 0.0,
            last_beat_floor: 0.0,
            seq: SeqState::new(),
            story: StoryState::new(),
            cam_bar_pos: 0.0,
            speed_bounce: 0.0,
            breath_bounce: 0.0,
            dragging: false,
            cursor: (0.0, 0.0),
            fullscreen: false,
            last_env_req: None,
            hdr: HdrState {
                hdr_path: None,
                // Start matched to the initial gens (s.hdr_gen = 0, local = 0) so the
                // FIRST frame is not seen as a change — otherwise a stale sidecar path
                // persisted in $TMPDIR from a prior session would auto-load on open. The
                // HDR loads only on an in-session gen bump ('O' key / editor button).
                last_hdr_gens: (0, 0),
                local_hdr_gen: 0,
                hdr_enabled: false,
                hdr_max: 1.0,
                last_hdr_ipc: false,
                hdr_wide: false,
                last_hdr_wide: false,
            },
            // Start at 0 = Shared's default, so the first frame doesn't spuriously
            // reload (no folder loaded yet → the neutral built-in set stays).
            last_material_gen: 0,
            last_material_procedural: false,
            last_anim_bake: 0.0,
            pathtrace_on: false,
            last_pathtrace_ipc: false,
            pathtrace_spp: 0,
            pt_prev_vp: Mat4::IDENTITY.to_cols_array_2d(),
            pt_prev_size: (0, 0),
            pt_prev_content: PT_CONTENT_NONE,
            nrc_cache: None,
            nrc_key: (0, 0),
            nrc_loss: 0.0,
            nrc_state: 0,
            last_msaa: 1,
            frame_guide: false,
            last_guide_ipc: false,
            record: RecordState {
                recorder: None,
                toggle_pending: false,
                perfect_pending: false,
                fixed: false,
                bars: 8,
                start_beat: 0.0,
                fps: recorder::DEFAULT_FPS,
                chunk_armed: false,
                chunk_arm_pending: false,
                chunk_phrase_beats: 8.0,
                chunk_grid_offset: 0.0,
                chunk_bpm: 120.0,
                chunk_index: 0,
                chunk_session: None,
                chunk_bar: None,
                pending_finalizers: Vec::new(),
                hud: None,
                error: None,
                note: None,
            },
            mods_shift: false,
            overlay_on: false,
            last_overlay_ipc: false,
            overlay_handle: String::new(),
            overlay_title: None,
            axes_solids: Vec::new(),
            box_lines: Vec::new(),
            chamber: ChamberDecor {
                surfs: Vec::new(),
                lines: Vec::new(),
                beads: Vec::new(),
                cam_right: [1.0, 0.0, 0.0],
                cam_up: [0.0, 1.0, 0.0],
                material: [0.0, 0.8, 0.25, 1.5],
                opacity: 0.85,
            },
            decor_on: true,
            // Sentinel ≠ any real gen, so the first frame always does an initial read of a
            // persisted overlay sidecar (handle/title) even when overlay_gen is still 0.
            last_overlay_gen: u32::MAX,
            terrain_noise: render::terrain_gen_noise(0, 1),
            terrain_key: (0, 1),
            terrain_time: 0.0,
            terrain_day: 0.0,
            sky_time: 0.0,
            wall_time: 0.0,
            prev_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            frame_ms: 1000.0 / 60.0,
            cpu_ms: 0.0,
            auto_scale: 1.0,
            frame_index: 0,
            applied_scale: 1.0,
            reader_retry: 0,
            feedback: ipc::FeedbackWriter::create().ok(),
            last_render_dims: (0, 0),
            particle_aura: ParticleAura {
                vel_grid: math::VelGrid::new([1, 1, 1], Vec3::splat(-1.0), Vec3::splat(1.0)),
                vel_upload: Vec::new(),
                node_samples: Vec::new(),
                prev_node_pos: Vec::new(),
                node_vels: Vec::new(),
                seed: 0,
                key: (u32::MAX, 0),
            },
            fluid_grids: FluidGrids {
                dye: math::VelGrid::new([1, 1, 1], Vec3::splat(-1.0), Vec3::splat(1.0)),
                dye_upload: Vec::new(),
                occ: math::VelGrid::new([1, 1, 1], Vec3::splat(-1.0), Vec3::splat(1.0)),
                occ_upload: Vec::new(),
                glow: math::VelGrid::new([1, 1, 1], Vec3::splat(-1.0), Vec3::splat(1.0)),
                glow_upload: Vec::new(),
            },
            maxdip_phase: 0.0,
            rms_hist: Vec::new(),
        }
    }

    /// #554 T1 — the mirror pass, run after the host's own frame.
    ///
    /// Deliberately *after*, not instead of: the separate visual window is the projector path
    /// and must be unaffected by whether anyone is mirroring it.
    ///
    /// #593 Tier 4 — full Organon only, and gated rather than left as an empty function on
    /// purpose: an inert stub still lets Mind's path *name* the mirror, and "nothing on Mind's
    /// path names it" is exactly what the mind-edition build compiling is supposed to prove.
    /// `bin/visual.rs`'s single call site carries the matching gate.
    #[cfg(not(feature = "mind-edition"))]
    pub fn pump_mirror_after_frame(&mut self) {
        self.pump_mirror();
    }

    /// #554 Tier 1 — publish one frame to the editor's viewport, if it is asking for one.
    ///
    /// Returns immediately when the toggle is off, and **drops** the mirror's resources when it
    /// goes off — the texture, the staging buffer and the ring file all go away, so a closed
    /// viewport costs nothing rather than merely doing nothing. `Mirror`'s docs carry the cost
    /// argument for when it is on.
    #[cfg(not(feature = "mind-edition"))]
    fn pump_mirror(&mut self) {
        if !self.mirror_want {
            // Dropping the writer leaves the file on disk but with `write_seq` frozen; the
            // editor's reader simply stops seeing new frames. Re-enabling recreates and rezeroes
            // it, so no stale frame can survive the round trip.
            self.mirror = None;
            self.mirror_tick = 0;
            return;
        }
        let Some(gfx) = self.gfx.as_ref() else { return };

        // Pace it. The `+ 1` ordering means the very first frame after enabling publishes
        // immediately rather than after a visible delay.
        self.mirror_tick = self.mirror_tick.wrapping_add(1);
        if self.mirror_tick % MIRROR_EVERY != 1 {
            return;
        }

        let (w, h) = (frame_ring::MIRROR_W, frame_ring::MIRROR_H);
        if self.mirror.is_none() {
            let Ok(writer) = frame_ring::FrameRingWriter::create() else { return };
            let texture = gfx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport-mirror"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                // Single-sample: `render_to_texture` requires it (MSAA resolves before the
                // composite regardless, so the scene is still anti-aliased).
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: MIRROR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let padded_bpr = recorder::padded_bytes_per_row(w, 4);
            let staging = gfx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport-mirror-readback"),
                size: (padded_bpr as u64) * (h as u64),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.mirror =
                Some(Mirror { writer, texture, staging, padded_bpr, cpu: Vec::new() });
        }

        // Draw the world into our texture. Identical scene; SDR because `presented == false`.
        let texture = self.mirror.as_ref().map(|m| m.texture.clone());
        let Some(texture) = texture else { return };
        self.render_to_texture(&texture, (w, h), MIRROR_FORMAT);

        let Some(gfx) = self.gfx.as_ref() else { return };
        let Some(m) = self.mirror.as_mut() else { return };

        let mut enc = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("mirror-copy") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &m.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &m.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(m.padded_bpr),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gfx.queue.submit([enc.finish()]);

        // Synchronous map. The recorder goes to real trouble to avoid this (async map + a worker
        // thread) because it runs at full frame rate; at MIRROR_EVERY pacing the simple version
        // is honest and the stall is affordable. If Tier 3 raises the rate, this is the first
        // thing that has to become async.
        m.staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gfx
            .device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None });

        let row = (w as usize) * 4;
        let padded = m.padded_bpr as usize;
        m.cpu.clear();
        m.cpu.reserve(row * h as usize);
        if let Ok(view) = m.staging.slice(..).get_mapped_range() {
            // Drop the row padding on the way out: the ring stores tightly-packed rows, so the
            // editor can hand the slice straight to `ColorImage` with no stride arithmetic.
            for y in 0..h as usize {
                let at = y * padded;
                m.cpu.extend_from_slice(&view[at..at + row]);
            }
        }
        drop_mapped(&m.staging);
        if m.cpu.len() == row * h as usize {
            m.writer.write_frame(w, h, &m.cpu);
        }
    }

    /// Draw one frame into a texture the caller owns (#541 S2 T3).
    ///
    /// The world is identical to `render()`: same generators, same passes, same
    /// MSAA, same capture/overlay/HUD layers. What is skipped is everything that
    /// only means something for a window — acquiring and presenting a swapchain
    /// image, "lock window to output", the window title, and the macOS EDR
    /// tagging (so an offscreen frame composites SDR; see `frame_hdr_max`).
    ///
    /// `texture` must be `RENDER_ATTACHMENT` (plus `TEXTURE_BINDING` if whoever
    /// owns it wants to sample it), **single-sample**, and exactly `size` /
    /// `format`. MSAA is unaffected by the choice of target: the scene is
    /// multisampled in the renderer's own `Rgba16Float` target and resolved long
    /// before the composite, which always writes a single-sample image here.
    ///
    /// The composite / FX / temporal pipelines are rebuilt on the first frame a
    /// new `format` is seen and then cached (`Gfx::out_format`), so alternating
    /// two formats frame-by-frame would thrash — pick one per target.
    ///
    /// Its consumer *was* the frame mirror (#554 T1). **#593 Tier 4 gated that away and did not
    /// replace it**: route C's editor renders straight into the swapchain image through
    /// `render_into`, so it never needs a texture of its own. So in a `mind-edition` build of
    /// `bin/visual.rs` — where `mod world` is private — this method is genuinely unreachable.
    ///
    /// It stays **ungated** because it is a real world capability rather than mirror plumbing:
    /// the #541 S2 T3 offscreen seam, the shape `render_into` was factored out of, and what any
    /// future offscreen consumer (a lens pane, a thumbnail, an export) would call. The `allow`
    /// is scoped to the one build where it is dead, so the default build still reports it if
    /// its last real caller ever goes.
    #[cfg_attr(feature = "mind-edition", allow(dead_code))]
    pub fn render_to_texture(
        &mut self,
        texture: &wgpu::Texture,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) {
        // An offscreen frame is never presented, so it gets no EDR headroom, no wide-gamut tag
        // and no interface painted over it — the three things that are a *display's* to grant.
        self.render_into(FrameTarget {
            texture,
            size,
            format,
            presented: false,
            hdr_max: 1.0,
            wide_gamut: false,
            ui_scale_factor: None,
        });
    }

    /// Draw one frame into the caller's texture, and report what it wants the host to do
    /// afterwards (#572 stage 3 — see [`FrameTarget`] / [`FrameRequests`]).
    pub fn render_into(&mut self, target: FrameTarget) -> FrameRequests {
        let mut requests = FrameRequests::default();
        self.frame_body(target, &mut requests);
        requests
    }

    /// The frame itself. Split from [`render_into`] for one reason: the body has a dozen early
    /// `return`s (no snapshot, occluded, nothing to draw), and threading a return *value*
    /// through every one of them would have been a dozen chances to drop a request on a path
    /// nothing here can test. It writes through `requests` instead and keeps returning `()`.
    fn frame_body(&mut self, target: FrameTarget, requests: &mut FrameRequests) {
        // Re-open the channel if the plugin started writing after we launched —
        // but throttled to ~1 Hz (#174 T2): with no writer (the standalone visual)
        // this was an open + fstat + mmap + munmap syscall burst at 60 Hz forever.
        // Also read the snapshot ONCE (it was read twice per frame).
        let mut s = self.reader.read();
        if s.seq == 0 {
            self.reader_retry += 1;
            if self.reader_retry >= 60 {
                self.reader_retry = 0;
                self.reader = ipc::Reader::open();
                s = self.reader.read();
            }
        } else {
            self.reader_retry = 0;
        }
        // #554 T1 — latch the editor's viewport toggle while the snapshot is in hand. Reading it
        // again in `render` would reintroduce the twice-per-frame read this function removed.
        // #593 Tier 4 — full Organon only. In a mind-edition build nothing ever stamps
        // `mindview[3]`, so the latch would read a constant `false` forever.
        #[cfg(not(feature = "mind-edition"))]
        {
            self.mirror_want = s.mindview_mirror();
        }

        // Editor's Renderer controls drive HDR via IPC flags. Apply only on a
        // change (edge), and only once the GPU exists, so the 'H' key's local state
        // isn't overwritten every frame and a pending request survives startup.
        let hdr_want = s.hdr_output != 0;
        let wide_want = s.hdr_wide != 0;
        if self.gfx.is_some() && (hdr_want != self.hdr.last_hdr_ipc || wide_want != self.hdr.last_hdr_wide) {
            let hdr_edge = hdr_want != self.hdr.last_hdr_ipc;
            let prev_wide = self.hdr.last_hdr_wide;
            self.hdr.last_hdr_ipc = hdr_want;
            self.hdr.last_hdr_wide = wide_want;
            self.hdr.hdr_wide = wide_want;
            let _ = prev_wide; // (the #582 wide-gamut trace is gone; the finding was withdrawn)
            if hdr_edge {
                self.hdr.set_hdr(hdr_want); // the host applies the swap + colorspace next frame
            }
            // A colorspace-only change (wide-gamut toggled while HDR is on) used to re-tag the
            // metal layer from here. That is the host's layer; it re-asserts when it sees
            // `hdr_request()` change, and the new headroom arrives on the next frame's target.
        }

        // Path tracer (#200 Tier 4): the editor's Ray-Tracing-card checkbox drives
        // the ground-truth mode via IPC. Apply only on a CHANGE (edge) so the 'P'
        // key's local toggle isn't overwritten every frame (last-touched-wins);
        // restart the progressive accumulation on the change.
        let pt_want = s.pathtrace_on != 0;
        if pt_want != self.last_pathtrace_ipc {
            self.last_pathtrace_ipc = pt_want;
            self.pathtrace_on = pt_want;
            self.pathtrace_spp = 0;
        }

        // MSAA sample count (rebuilds scene pipelines + multisample targets).
        let msaa_want = s.msaa.max(1);
        if msaa_want != self.last_msaa {
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.renderer.set_sample_count(&gfx.device, msaa_want);
                self.last_msaa = msaa_want;
            }
        }

        // #430 perfect capture: decide THIS frame's clock mode before the animation
        // integrates, so the very first captured frame already uses the fixed step (the
        // recorder is actually started later in this same render() pass). Only engage on a
        // START (recorder currently None); a failed start reverts it below.
        let record = &mut self.record;
        if record.toggle_pending && record.recorder.is_none() && record.perfect_pending {
            record.fixed = true;
        }

        // Advance the beat clock (frame-rate independent; PLL-locked to host).
        let now = Instant::now();
        let mut dt = (now - self.last_frame).as_secs_f64().clamp(0.0, 0.1);
        self.last_frame = now;
        // #430 perfect capture: while recording in fixed-timestep mode, drive the ENTIRE
        // animation (beat clock, camera, phases, sway…) at exactly 1/FPS per frame instead
        // of wall-clock dt. Every captured frame is then one 60fps tick of motion apart, so
        // playback is perfectly even and readback latency can't jitter it. The window itself
        // may then animate off real-time during a take (e.g. 2× at 120Hz), but the file is
        // correct — its timeline is animation time, not wall time.
        if record.fixed {
            dt = 1.0 / record.fps.value().max(1.0);
        }
        advance_beat_clock(&mut self.beat_pos, &mut self.beat_host_pos_prev, &s, dt, record.fixed);

        // Auto-orbit: integrate the camera phase, kicking velocity on each beat
        // crossing of the (host-locked) clock. `camera = [path, speed, kick,
        // damping]`; bpm comes from the same source the beat clock used.
        let beat_floor = self.beat_pos.floor();
        let kicks = (beat_floor - self.last_beat_floor).max(0.0);
        self.last_beat_floor = beat_floor;
        let bpm = active_bpm(&s);
        let dt_beats = dt * bpm.max(0.0) / 60.0;
        // Beat momentum (#307) is opt-out: when off, the kick is suppressed so the
        // camera glides on the bar clock instead of lurching with the audio.
        let momentum = s.cam_clock[1] > 0.5;
        advance_camera(
            &mut self.cam_phase,
            &mut self.cam_vel,
            s.camera[1] as f64,                             // base speed (cycles/beat)
            if momentum { s.camera[2] as f64 } else { 0.0 }, // kick (gated)
            s.camera[3] as f64,                             // damping (per-beat retention)
            dt_beats,
            kicks,
        );

        // Camera shot sequencer (#307): advance the bar clock and step the active
        // move on each `bars_per_shot`-bar boundary. Bookkeeping runs every frame
        // (cheap) so enabling it mid-track picks up cleanly; its output is only used
        // when `cam_seq[0]` is on (see the offset section below).
        let beats_per_bar = (s.cam_dolly[3] as f64).max(1.0);
        self.cam_bar_pos = self.beat_pos / beats_per_bar;
        self.seq.step(
            self.cam_bar_pos,
            (s.cam_seq[1] as f64).max(1.0),
            s.cam_seq[2] as u32,
            s.cam_frame[3], // hold probability (Tier 2)
        );
        // Storyboard (#307 Tier 3): plays an authored playlist; overrides the auto
        // sequencer when enabled. Bookkeeping runs every frame (cheap).
        self.story.step(self.cam_bar_pos, &s.cam_story);
        // Phrase-locked facing (#307 Tier 2): on a shot change, snap the move phase
        // to a canonical start so the camera faces consistently on the downbeat.
        let shot_changed = if s.cam_story[0] > 0.5 {
            self.story.just_changed
        } else {
            s.cam_seq[0] > 0.5 && self.seq.just_changed
        };
        if s.cam_frame[4] > 0.5 && shot_changed {
            self.cam_phase = 0.0;
            self.cam_vel = 0.0;
        }

        // AI Performer (#317 Tier 1): edge-detect the plugin-published chat/plan/release
        // counters, then apply the agent override lane onto `s` BEFORE `ParamValues` is
        // built (so geometry holds flow into `draw_tissue`) and before `build_uniforms`
        // (so look holds reach the shader) — the pulse-routing precedent. Inert when the
        // agent block is all zeros (standalone visual / no model).
        Self::step_agent(
            &mut self.performer,
            &mut self.cmd_chan,
            record,
            &mut self.geom,
            self.frame_ms,
            self.cpu_ms,
            &mut s,
        );

        let mut pv = ParamValues {
            loop_count: Vec3::new(s.loop_count[0], s.loop_count[1], s.loop_count[2]),
            loop_count_q: s.loop_count[3],
            rot_amp: Vec3::new(s.rot_amp[0], s.rot_amp[1], s.rot_amp[2]),
            trans_amp: Vec3::new(s.trans_amp[0], s.trans_amp[1], s.trans_amp[2]),
            trans_mod: Vec3::new(s.trans_mod[0], s.trans_mod[1], s.trans_mod[2]),
            scale_amp: s.scale_amp,
        };

        // The pulse envelope drives both the routing slots and the exposure/glow
        // pump. Its source is either the synthetic decaying beat impulse or the
        // live audio bass envelope (`pulse_source == 1`) — see `pulse_envelope`.
        let pulse_env = pulse_envelope(&s, self.beat_pos);

        // #248 hue cycle: the beat pulse advances the energized motes' hue (mn_hue_cycle),
        // so the vortex pulses through the colour wheel with the music. Wrapped to [0,1).
        // Rate 0 → reset the phase so the motes return to the fixed ember hue (not stuck at
        // the last accumulated offset); Pulse off just holds the phase (no advance).
        if s.mxforce3[2] > 0.0 {
            if s.pulse != 0 {
                self.hue_phase = (self.hue_phase
                    + pulse_env as f64 * s.mxforce3[2] as f64 * dt)
                    .rem_euclid(1.0);
            }
        } else {
            self.hue_phase = 0.0;
        }

        // Pulse routing: add the envelope to up to two target params (geometry on
        // `pv`, look on `s`). Active only while pulse is on.
        if s.pulse != 0 {
            let (at, ad) = (s.routing[0] as u32, s.routing[1]);
            let (bt, bd) = (s.routing[2] as u32, s.routing[3]);
            apply_mod(&mut s, &mut pv, at, ad * pulse_env);
            apply_mod(&mut s, &mut pv, bt, bd * pulse_env);
        }

        // Speed Pulse: a logarithmic kick to the global rotation speed (multiplies
        // the decade, where the routing's linear add was invisible). Driven by the
        // same pulse envelope but with its own attack/decay, so the bounce-back is
        // tunable independently. Returns 1.0 when inert (amount 0 / pulse off).
        let drive = if s.pulse != 0 { pulse_env } else { 0.0 };
        let speed_mult = speed_pulse_mult(
            &mut self.speed_bounce,
            drive,
            s.speed_pulse[0] as f64,          // amount (decades)
            s.speed_pulse[1] as f64 / 1000.0, // attack (s)
            s.speed_pulse[2] as f64 / 1000.0, // decay (s)
            dt,
        );

        // Breath: a universal pulse-driven scale of the whole scene, applied at the
        // view level in `build_uniforms` (so it works for every generator + surface
        // mode). Same pulse envelope, own attack/decay. Vec3::ONE when inert.
        let breath_scale = breath_scale_vec(
            &mut self.breath_bounce,
            drive,
            s.breath[0] as f64,          // amount (scale depth)
            s.breath[1] as f64 / 1000.0, // attack (s)
            s.breath[2] as f64 / 1000.0, // decay (s)
            dt,
        );

        let rot_func = FuncName::from_u32(s.rot_func);
        if s.animate != 0 {
            // Per-axis rotation speed lives in rot_mod[0..2] (incl. any pulse
            // routing applied above); inc_scale is the global multiplier (rot_mod[3]).
            // The clock is f64 and only lightly wrapped so continuous mode advances
            // smoothly for many hours without drift; pendulum's sin() is seamless
            // at the wrap because it's a whole number of τ periods.
            let speed = DVec3::new(s.rot_mod[0] as f64, s.rot_mod[1] as f64, s.rot_mod[2] as f64)
                * s.rot_mod[3] as f64
                * speed_mult;
            self.angle += speed;
            // Wind phase: continuous winding whose *velocity* the rotation waveform
            // shapes (always forward — `wind_velocity` clamps ≥ 0 — and mean ≈ 1, so
            // the global Speed still sets the rate). `cont_shape` 0 → velocity 1 and
            // wind tracks angle exactly, i.e. the old constant spin.
            let depth = s.cont_shape as f64;
            let vel = DVec3::new(
                wind_velocity(rot_func, self.angle.x, depth),
                wind_velocity(rot_func, self.angle.y, depth),
                wind_velocity(rot_func, self.angle.z, depth),
            );
            self.wind_phase += speed * vel;
            let wrap = std::f64::consts::TAU * 1.0e6;
            for c in [
                &mut self.angle.x, &mut self.angle.y, &mut self.angle.z,
                &mut self.wind_phase.x, &mut self.wind_phase.y, &mut self.wind_phase.z,
            ] {
                if c.abs() > wrap {
                    *c %= std::f64::consts::TAU;
                }
            }
        }

        // Rotation reads the wave-shaped wind phase in continuous mode, the linear
        // angle in pendulum mode (translation always uses the linear angle).
        let continuous = s.rot_amp[3] != 0.0;
        let rot_phase = if continuous { self.wind_phase } else { self.angle };

        // Bioluminescence phase clocks (free-running, real-time). `color_phase`
        // flows the palette sweep; `ripple_phase` drives the travelling emissive
        // band (read into the uniforms below). Wrapped to 0..1.
        self.color_phase = (self.color_phase + s.bio[0] as f64 * dt).rem_euclid(1.0);
        self.ripple_phase = (self.ripple_phase + s.bio[2] as f64 * dt).rem_euclid(1.0);

        // Membrane (mode 4): the lofted sheet is its own mesh; when "show strands"
        // is on we also draw the boundary strands as swept tubes.
        let membrane_mode = s.surface_mode == 4;
        // Mutable: a mixed-topology rails transition forces it on for a frame
        // so the un-lofted world stays visible (#187 Tier 3, see the Rails arm).
        let show_strands = membrane_mode && s.membrane[1] > 0.5;
        // #membrane-arms: skin each arm (strand) as its own closed finger with gaps
        // (membrane[2]); close the loft seam at a 360° wrap (membrane[3]); Skin-Arms
        // build path (membrane_fx[1]): 0 = capsule Impostors, 1 = welded Mesh.
        let membrane_arms = membrane_mode && s.membrane[2] > 0.5;
        let membrane_close = s.membrane[3] > 0.5;
        let membrane_arm_mesh = s.membrane_fx[1] > 0.5;
        let trans_func = FuncName::from_u32(s.trans_func);
        let palette = s.surface_fx[6] as u32;
        let generator = GeneratorMode::from_u32(s.generator);

        // #226 Tier 3/4/5: ingest a graph when the editor bumps `nn_gen` — read the
        // sidecar path + JSON and AUTO-DETECT the format: an `"attention"` file is a
        // transformer tensor (Tier 5 → `neural_attn_data`); a `"weights"`/`"layers"` file
        // is a trained **MLP** (Tier 4 → `neural_mlp`, its forward pass builds the graph
        // each frame); anything else is a **connectome** (Tier 3 → `neural_loaded`).
        // (Attention is checked first — its schema also carries a `"layers"` key.)
        if s.nn_gen != self.last_nn_gen {
            self.last_nn_gen = s.nn_gen;
            let extent = s.neural_net[6].max(1.0);
            let json = std::fs::read_to_string(ipc::connectome_sidecar_path())
                .ok()
                .and_then(|p| std::fs::read_to_string(p.trim()).ok());
            if let Some(json) = json {
                if json.contains("\"attention\"") {
                    match math::neural_attention_from_json(&json) {
                        Ok(a) => {
                            eprintln!(
                                "neural: attention loaded — {} layers, {} heads, {} tokens",
                                a.n_layers, a.n_heads, a.n_tokens
                            );
                            self.neural_attn_data = Some(a);
                            self.neural_mlp = None;
                            self.neural_loaded = None;
                            self.atlas_is_loaded = false;
                            self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                        }
                        Err(e) => eprintln!("neural: attention load failed: {e}"),
                    }
                } else if json.contains("\"weights\"") || json.contains("\"layers\"") {
                    match math::neural_mlp_from_json(&json) {
                        Ok(m) => {
                            eprintln!(
                                "neural: MLP loaded — layers {:?}, {} units",
                                m.layers,
                                m.node_count()
                            );
                            self.neural_mlp = Some(m);
                            self.neural_loaded = None;
                            self.atlas_is_loaded = false;
                            self.neural_attn_data = None;
                            self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                        }
                        Err(e) => eprintln!("neural: MLP load failed: {e}"),
                    }
                } else {
                    match math::neural_graph_from_json(&json, extent) {
                        Ok(g) => {
                            eprintln!(
                                "neural: connectome loaded — {} nodes, {} edges",
                                g.nodes.len(),
                                g.edges.len()
                            );
                            self.neural_loaded = Some(g);
                            self.atlas_is_loaded = false;
                            self.neural_mlp = None;
                            self.neural_attn_data = None;
                            self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                        }
                        Err(e) => eprintln!("neural: connectome load failed: {e}"),
                    }
                }
            }
        }

        // #476 Tier 2b: ingest an authorable creature body plan when the editor bumps
        // `creature_gen` — read the sidecar path + JSON, parse it, and replace the
        // built-in `form` plan (the connectome/`nn_gen` pattern). A failed load clears
        // back to the built-in plan rather than keeping a stale one.
        if s.creature_gen != self.last_creature_gen {
            self.last_creature_gen = s.creature_gen;
            let json = std::fs::read_to_string(ipc::creature_sidecar_path())
                .ok()
                .and_then(|p| std::fs::read_to_string(p.trim()).ok());
            match json.as_deref().map(math::parse_creature_spec) {
                Some(Ok(plan)) => {
                    eprintln!("creature: body plan loaded — {} primitives", plan.len());
                    self.creature_loaded = Some(plan);
                }
                Some(Err(e)) => {
                    eprintln!("creature: body-plan load failed: {e}");
                    self.creature_loaded = None;
                }
                None => self.creature_loaded = None,
            }
        }

        // #367 Tier 1 (visible-mind specimen): when the editor bumps `mind[1]`
        // (`model_gen`), read the `.gguf` path from the model sidecar, parse the
        // GGUF *header* (no weights), build the architecture topology, and feed the
        // SAME `neural_loaded` slot the connectome path fills — so every Surface /
        // Material / palette applies for free. Select the Neural Network generator +
        // topology = Connectome to see it.
        // #520 — `mind[2]` selects the mind GEOMETRY only: 0 = the #367 T1 architecture
        // specimen, 1 = the #507 T1 embedding galaxy, 2 = the #147 T3 Delta lens. A
        // change between any two of them has to rebuild `neural_loaded` from the
        // already-loaded model, so it edge-detects here alongside `model_gen`. Default
        // 0 → never fires → today's behaviour.
        // "Live" is not a mode — the glow rides the activation ring whenever frames are
        // arriving (see the `topo == 5` seam), so Generate never yanks the view out from
        // under you. ⚠️ That seam is gated to view 0, which is also the guarantee the
        // Delta lens rests on: a **measured** training-time quantity cannot be silently
        // repainted by the **proxy** generation-time one, because they cannot share a
        // picture.
        let mind_view = math::mind_view_mode(s.mind[2]);
        let view_changed = mind_view != self.last_mind_view;
        self.last_mind_view = mind_view;
        // `extent` is baked into the graph when it is built, so a slider move has to
        // rebuild or it does nothing at all (the #520 bug). The galaxy rebuild is cheap
        // because `build_mind_graph` rescales the CACHED projection.
        let mind_extent = s.neural_net[6].max(1.0);
        let extent_changed = mind_extent != self.last_mind_extent;
        self.last_mind_extent = mind_extent;

        let model_gen = s.mind[1] as u32;
        if model_gen != self.last_model_gen {
            self.last_model_gen = model_gen;
            let extent = s.neural_net[6].max(1.0);
            let path = std::fs::read_to_string(ipc::model_sidecar_path())
                .ok()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty());
            // A failed load must NOT leave the prior specimen/connectome on screen
            // (Bugbot #390): `last_model_gen` still advances (no retry-loop), but on a
            // missing/empty sidecar path OR a GGUF parse error we CLEAR the specimen
            // graph — drop back to the procedural/empty topology (`neural_loaded = None`
            // → `unwrap_or_default()` downstream) rather than presenting stale geometry
            // as if the new model had loaded. The Mind→"Model / Specimen" card surfaces
            // the failure via the editor's own parse ("parse failed: …").
            let load_result = match path.as_deref() {
                Some(p) => organon_core::gguf::parse_file(p)
                    .map(|h| (p.to_string(), h))
                    .map_err(|e| e.to_string()),
                None => Err("no model path (sidecar missing or empty)".to_string()),
            };
            match load_result {
                Ok((loaded_path, h)) => {
                    eprintln!(
                        "mind: model loaded from {} — arch {}, {} layers, {} heads",
                        loaded_path, h.arch, h.n_layers, h.n_heads
                    );
                    // A new model invalidates the cached projection.
                    self.mind_galaxy = None;
                    self.neural_loaded = build_mind_graph(
                        &loaded_path,
                        &h,
                        mind_view,
                        extent,
                        &mut self.mind_galaxy,
                        &mut self.mind_delta,
                    );
                    self.mind_model = Some((loaded_path, h));
                    self.atlas_is_loaded = false;
                    self.neural_mlp = None;
                    self.neural_attn_data = None;
                    self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                }
                Err(e) => {
                    eprintln!(
                        "mind: GGUF load failed{} — clearing specimen graph: {e}",
                        path.as_deref().map(|p| format!(" for {p}")).unwrap_or_default()
                    );
                    // Clear rather than keep a stale specimen/connectome.
                    self.neural_loaded = None;
                    self.mind_model = None;
                    self.mind_galaxy = None;
                    self.atlas_is_loaded = false;
                    self.neural_mlp = None;
                    self.neural_attn_data = None;
                    self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                }
            }
        } else if view_changed || extent_changed {
            // The model is already parsed, so swapping Specimen <-> Galaxy — or moving
            // the extent slider — just rebuilds the graph from what we cached. Both
            // views OWN `neural_loaded`, and the live glow overwrites `node_scalar` on
            // top of it per frame, so rebuilding here is safe while generating.
            if let Some((p, h)) = self.mind_model.clone() {
                self.neural_loaded = build_mind_graph(
                    &p,
                    &h,
                    mind_view,
                    mind_extent,
                    &mut self.mind_galaxy,
                    &mut self.mind_delta,
                );
                self.atlas_is_loaded = false;
                self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
            }
        }

        // #423 Tier 1 — the atlas. When `atlas[0]` (atlas_gen) bumps, read the atlas
        // sidecar (the editor's scanned + derived design points), build the
        // design-space constellation into the SAME `neural_loaded` slot the specimen
        // uses, and cache the points + profile for the roofline inset. A failed /
        // empty read clears the atlas (points empty → inset hides) but leaves any
        // existing specimen graph alone (last scan wins, like the model seam).
        let atlas_gen = s.atlas[0] as u32;
        if atlas_gen != self.last_atlas_gen {
            self.last_atlas_gen = atlas_gen;
            let extent = s.neural_net[6].max(1.0);
            match std::fs::read_to_string(ipc::atlas_sidecar_path())
                .ok()
                .and_then(|j| serde_json::from_str::<math::AtlasDoc>(&j).ok())
            {
                Some(doc) if !doc.points.is_empty() => {
                    let g = math::design_space_constellation(&doc.points, extent);
                    eprintln!(
                        "atlas: {} models → {} nodes, {} edges (profile {}, ctx {})",
                        doc.points.len(),
                        g.nodes.len(),
                        g.edges.len(),
                        doc.profile.name,
                        doc.context_tokens
                    );
                    self.atlas_points = doc.points;
                    self.atlas_profile = doc.profile;
                    self.neural_loaded = Some(g);
                    self.atlas_is_loaded = true;
                    self.neural_mlp = None;
                    self.neural_attn_data = None;
                    self.neural_load_gen = self.neural_load_gen.wrapping_add(1);
                }
                _ => {
                    eprintln!("atlas: sidecar missing/empty — clearing design points");
                    self.atlas_points.clear();
                }
            }
        }

        // Scenery ride (#187 pivot): the camera + input reinterpret while the
        // Zone corridor is on (forward flight down −Z; drag = a bore-clamped
        // X/Y offset) — scenery is a LAYER now, concurrent with any generator.
        // Scenery on for Zone (1) OR Terra (2, #206).
        self.rails_active = matches!(s.scenery[0] as u32, 1 | 2);
        if !self.rails_active {
            self.rails_was_on = false;
        }
        self.rails_bore = s.rails[1].max(0.25);

        // Generic generator phase: advanced by the global Speed (incl. the
        // speed-pulse multiplier → beat-reactive). Frenet feeds it into κ/τ so the
        // curve winds/unwinds in time; Original ignores it (uses its own clock).
        if s.animate != 0 {
            self.gen_phase += s.rot_mod[3] as f64 * speed_mult;
        }

        // #248 Tier 3: the audio-dipole's oscillation clock. With the drive on, it
        // advances like gen_phase but scaled by pitch → the spectral centroid
        // (`audio[6]`): brighter music breathes faster. A declared, huge reduction of
        // the true audio rate (we render the field's watchable oscillation, never the
        // 20 Hz–20 kHz carrier). Drive off → hard-synced to gen_phase, so every Maxwell
        // phase read stays byte-identical.
        if s.maxwell[22] > 0.5 {
            // Tempo Sync (PR #320), now centralized on the shared Duo-Field oscillation
            // clock. Maxwell AND Acoustic both read `maxdip_phase` for their geometry,
            // aura/energy-cloud, and energy bakes; driving the sync HERE (not only in the
            // Maxwell geometry arm) locks every one of those reads to the PLL beat clock —
            // one full field there-and-back per note division (maxwell[23]) — so the field
            // lines, the energy cloud, and the B swirl all reverse TOGETHER on one clock.
            // Overrides the free-run + the #248 audio pitch-rate while on.
            self.maxdip_phase = maxwell_osc_phase(
                self.beat_pos,
                OscDivision::from_u32(s.maxwell[23] as u32),
                beats_per_bar as f32,
            ) as f64;
        } else if s.audiodip[0] != 0.0 {
            if s.animate != 0 {
                let rate = 1.0 + (s.audiodip[7].max(0.0) * s.audio[6].clamp(0.0, 1.0)) as f64 * 4.0;
                self.maxdip_phase += s.rot_mod[3] as f64 * speed_mult * rate;
            }
        } else {
            self.maxdip_phase = self.gen_phase;
        }
        // Loudness history (newest first, ~1 s at 60 fps) for the Tier-3
        // retarded-amplitude waveform shells.
        self.rms_hist.insert(0, s.audio[5].max(0.0));
        self.rms_hist.truncate(64);

        // The strand generators (Frenet, DNA) have no sheet lofter yet (fast-follow),
        // so Membrane mode renders their strands as swept tubes instead of a sheet,
        // and the sheet mesh isn't drawn for them.
        let strand_gen = generator != GeneratorMode::Original;
        let strand_membrane_fallback = strand_gen && membrane_mode;
        // Neural Tissue (7, #260 Tier 1): closed anatomical primitives. For the
        // Neural Network generator it lowers to soma/capsule/bouton sub-batches
        // (`neural_batches`); for every other generator it degrades to closed
        // capped capsules (like Swept Tubes but with the capsule mesh).
        let neural_tissue = s.surface_mode == 7;
        // Plexus (9): a proximity "web" over the node cloud. It doesn't trip any of
        // the flow-aligned / tube / weld / membrane / metaball / voxel gates (those
        // key off other ordinals), so each generator emits plain node cubes into
        // `self.geom.instances` exactly as in the Original surface — which the post-match
        // pass below then rebuilds into struts + markers. Generator-agnostic.
        // (Splat took ordinal 8 on the main merge, so Plexus is 9.)
        let plexus = s.surface_mode == 9;
        // Flow-Aligned (1) and Swept Tubes (2) bridge node→successor; Membrane with
        // strands shown reuses the swept-tube look for those strands. Neural Tissue
        // also bridges (its non-graph fallback renders closed capsules).
        // Volumetric field-lines (#348/#349): the Acoustic (radiating) or Maxwell field
        // renders as a dense cloud of thin glowing streamlines (the tube-mode flow,
        // without chunky tubes). Force the swept-tube render + suppress the Volume
        // raymarch for it.
        let fv_lines_active = s.fieldvol[5] > 0.5
            && match GeneratorMode::from_u32(s.generator) {
                GeneratorMode::Acoustic => s.acoustic2[0] <= 0.5, // radiating (not cavity)
                GeneratorMode::MaxwellField => true,
                _ => false,
            };
        // Plexus needs plain node cubes (each instance's translation IS a node centre)
        // so the post-match graph pass reads real nodes, not strut midpoints — so it
        // must never trip flow-aligned/tube, not even via `fv_lines_active` (which can
        // co-occur with Plexus on the Acoustic/Maxwell field-line generators).
        let flow_aligned = (s.surface_mode == 1 || s.surface_mode == 2 || neural_tissue || show_strands
            || fv_lines_active)
            && !plexus;
        let tube = (s.surface_mode == 2 || neural_tissue || show_strands || strand_membrane_fallback
            || fv_lines_active)
            && !plexus;
        // Membrane sheet: Original builds it from the cube-field; the Grid-topology
        // generators (Frenet / DNA / Harmonic) loft it from their strands via
        // `loft_membrane` (set inside their match arms, #60); Streamlines (Attractor)
        // leaves it false and degrades to the swept-tube fallback above.
        let mut draw_membrane_mesh =
            membrane_mode && generator == GeneratorMode::Original && !membrane_arms;
        // Boids creature form (#52): -1 = none, else the creature-mesh kind. Set in
        // the Boids arm; when ≥ 0 it overrides the surface mode (no metaball/voxel/
        // membrane) and the renderer draws one creature per agent.
        let mut boids_creature: i32 = -1;

        // Neural latent walk (#200 Tier 1/1b): a triangle-wave morph seed A→B whose
        // rate is walk-cycles per beat off the PLL beat clock (0 = static/manual).
        // Resolved once here so both the strand generator (below) and the raymarch
        // params use the identical position.
        let neural_walk_resolved = {
            let ph = s.neural[3] as f64 + self.beat_pos * s.neural2[6] as f64;
            let frac = ph.rem_euclid(2.0);
            (if frac <= 1.0 { frac } else { 2.0 - frac }) as f32
        };

        // Neural Tissue (#260 Tier 1): default to no sub-batches; the Neural Network
        // arm sets it when it lowers to soma/capsule/bouton primitives.
        self.neural_batches = None;
        // Demo scene bench (#288): cleared each frame; the Demo arm refills it.
        self.demo_batches.clear();
        self.demo_lights.clear();
        // Membrane Skin-Arms Impostor capsules: cleared each frame; only the arms-
        // impostor path (Acoustic) refills it, so every other frame passes empty.
        self.arm_caps.clear();
        // Plexus impostor batches: cleared each frame; only the plexus impostor path
        // below refills them, so every other frame passes empty (no-op draw).
        let plex = &mut self.plexus;
        plex.batches = None; // set below only for Tier-1 plexus
        plex.node_caps.clear();
        plex.edge_caps.clear();
        // Plexus OVERLAY (outer shell around another surface): cleared each frame; the
        // overlay block below refills it only when the overlay is on + a base surface
        // emitted a node cloud. Off → these stay empty → byte-identical.
        plex.overlay_batches = None;
        plex.ov_insts.clear();
        plex.ov_itints.clear();

        // Generator dispatch (#42/#43): which algorithm produces the node field.
        // Both arms fill `self.geom.instances` + `self.geom.tints` (so every surface mode and
        // metaball, which reads instances, work) and return the scene bounds.
        // Contiguous Swept-Tubes (Swept Tubes surface mode + weld flag): build ONE
        // welded mesh per strand instead of the instanced per-segment cylinders.
        // `instances`/`tints` are cleared so the instanced draw + SSAO prepass skip
        // themselves; the renderer draws `swept_mesh` instead. The Original generator
        // re-derives its cube-field flow (`build_swept_tubes`); every Streamlines
        // generator welds its shared `gen_strands` (`weld_strands`, via `emit_strands`
        // at each `lower_strands` site), so welding works for Maxwell/DNA/attractors/…
        // Skin-Arms Mesh build folds into `weld`: every generator's existing swept-tube
        // weld path (Original's `build_swept_tubes`, the Grid generators' `weld_strands`
        // via `emit_strands`) then produces the per-arm capped fingers — no per-generator
        // arm code. The shell loft is skipped (guarded on `!membrane_arms`) and the arms-
        // Impostor build is handled generically post-match.
        let arms_mesh = membrane_mode && membrane_arms && membrane_arm_mesh;
        let weld = (s.surface_mode == 2 && s.tube[0] != 0.0) || arms_mesh;
        // The ray tracer's copy of the tube geometry is only needed when welded AND
        // some RT feature is live (else the extra lowering + upload is wasted). The
        // RT/PT gates below also require it non-empty, so plain welded views skip it.
        self.geom.rt_geo_wanted = weld
            && (self.pathtrace_on
                || s.rt[0] != 0.0
                || s.rt[2] != 0.0
                || s.rt2[0] != 0.0
                || s.rt2[5] != 0.0
                || s.rt3[0] != 0.0);
        // Cleared every frame; `emit_strands` refills it only when `rt_geo_wanted`,
        // and non-Streamlines generators leave it empty (RT then uses `self.geom.instances`).
        self.geom.rt_instances.clear();
        self.geom.rt_tints.clear();
        // organon#217 T1: emission is a glyph-frame thing; every other frame passes
        // the renderer an empty slice (= zero) — cleared here so a generator drawn
        // after the ring goes quiet cannot be handed a stale grid's phosphor.
        self.geom.emits.clear();
        // Welded node anchors for the node-driven systems (cleared every frame;
        // `emit_strands` refills only when `need_weld_nodes` AND welded). A node system
        // is live when the Particle Aura tier ≥ 1, the Fluid Ink is on, or the liquid
        // colliders are on — the same gates the aura/ink/liquid-collider paths use.
        self.geom.need_weld_nodes = s.particles[0] as u32 >= 1
            || s.fluidvis[0] != 0.0
            || (s.liquid[0] != 0.0 && s.liquid[8] != 0.0)
            // Node-set consumers that Contiguous mode otherwise starves (they read
            // `instances`, empty in welded mode): Voxel GI (#152 T3), emissive
            // many-lights (#167 T3), and the bounced-GI probe volume (#80 B). Feed
            // them the same per-segment node anchors the aura/ink/liquid use.
            || s.vxgi[0] != 0.0
            || s.manylight[0] != 0.0
            || s.gi[0] != 0.0
            // Plexus overlay reads the node cloud too — Contiguous welded Swept Tubes
            // clears `instances`, so it needs the welded per-segment anchors as well.
            || s.plexus_overlay[0] != 0.0;
        self.geom.node_insts_weld.clear();
        self.geom.node_tints_weld.clear();
        let caps = math::CapParams {
            end_cap: s.tube[1] != 0.0,
            round: s.tube[2],
            bevel: s.tube[3],
        };
        self.geom.tube_profile = s.tube_profile; // 1 = circle → 0 = sharp square
        self.geom.swept_mesh.clear(); // rebuilt below by whichever generator welds this frame
        // Origin mode (Original cube-field): Corner (0) vs Centered (1). Only the
        // Original generator's node builders read it; the strand generators ignore it.
        let centered = s.origin_mode != 0;
        let bounds = match generator {
            GeneratorMode::Original => {
                let b = if weld {
                    self.geom.instances.clear();
                    self.geom.tints.clear();
                    // RT geometry: the segmented cube/tube version so the ray tracer
                    // has something to trace while the raster draws the welded mesh —
                    // only when an RT feature is live (`rt_geo_wanted`).
                    if self.geom.rt_geo_wanted {
                        math::draw_tissue(
                            &pv,
                            rot_func,
                            trans_func,
                            self.angle,
                            rot_phase,
                            continuous,
                            flow_aligned,
                            tube,
                            centered,
                            palette,
                            self.color_phase as f32,
                            CUBE_CEILING,
                            &mut self.geom.rt_instances,
                            &mut self.geom.rt_tints,
                        );
                    }
                    math::build_swept_tubes(
                        &pv,
                        rot_func,
                        trans_func,
                        self.angle,
                        rot_phase,
                        continuous,
                        true, // tube colour: HSV sweep along each tube (matches Swept Tubes)
                        centered,
                        palette,
                        CUBE_CEILING,
                        caps,
                        self.geom.tube_profile, // 1 = circle → 0 = sharp square
                        &mut self.geom.swept_mesh,
                    )
                } else {
                    math::draw_tissue(
                        &pv,
                        rot_func,
                        trans_func,
                        self.angle,
                        rot_phase,
                        continuous,
                        flow_aligned,
                        tube,
                        centered,
                        palette,
                        self.color_phase as f32,
                        CUBE_CEILING,
                        &mut self.geom.instances,
                        &mut self.geom.tints,
                    )
                };
                if draw_membrane_mesh {
                    math::draw_membrane(
                        &pv,
                        rot_func,
                        trans_func,
                        self.angle,
                        rot_phase,
                        continuous,
                        centered,
                        palette,
                        self.color_phase as f32,
                        s.membrane[0] as u32, // weave id
                        membrane_close,       // close the 360° seam
                        &mut self.mem_pos,
                        &mut self.mem_norm,
                        &mut self.mem_col,
                        &mut self.mem_idx,
                    );
                }
                b
            }
            GeneratorMode::Frenet => {
                let fr = &s.frenet;
                math::frenet_strands(
                    FuncName::from_u32(fr[11] as u32), // κ/τ waveform
                    fr[0].max(1.0) as usize,           // strands
                    fr[1].max(2.0) as usize,           // nodes
                    fr[2],                             // step ds
                    fr[3], fr[4], fr[5],               // kappa, amp, freq
                    fr[6], fr[7], fr[8],               // tau, amp, freq
                    self.gen_phase as f32,             // animation phase
                    fr[9],                             // spread
                    fr[10],                            // thickness
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                // Grid topology → cube / flow-aligned / swept-tubes / metaball all
                // work. Membrane lofts a rippling ribbon across the strand bundle.
                if membrane_mode && !membrane_arms {
                    let mem = math::strands_to_mem(&self.geom.gen_strands);
                    let gx = mem.len();
                    math::loft_membrane(
                        &mem, gx, 1, 1, true, 0, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::Dna => {
                let d = &s.dna;
                math::dna_strands(
                    d[0] as u32,        // form
                    d[1].max(2.0) as usize, // base pairs
                    d[2],               // bp/turn (custom)
                    d[3],               // rise Å
                    d[4],               // radius Å
                    d[5],               // groove °
                    d[6] > 0.5,         // left-handed
                    d[7],               // sigma
                    d[8],               // superhelix radius Å
                    d[9] as u32,        // sequence seed
                    d[10],              // thickness
                    d[11],              // twist breathe amp
                    self.gen_phase as f32,
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                // Two backbones + base-pair rungs (Grid). Cube = beads, Swept Tubes =
                // backbones + rungs (canonical), Metaball = melted duplex. Membrane
                // lofts a sheet between the two backbones = the twisted-ribbon cartoon
                // (rungs + backbones still draw as tubes under it).
                if membrane_mode && !membrane_arms && self.geom.gen_strands.len() >= 2 {
                    let mem = math::strands_to_mem(&self.geom.gen_strands[0..2]);
                    math::loft_membrane(
                        &mem, 2, 1, 1, true, 0, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::Attractor => {
                let a = &s.attr;
                let field = a[0] as u32;
                let nseeds = (a[1] as usize).clamp(1, 64);
                let seedval = a[2] as i32;
                let spread = a[3] as f64;
                let dt = math::attractor_dt(field) * a[4].max(0.01) as f64;
                let trail_len = (a[5] as usize).clamp(2, 4096);
                let head_speed = a[6] as f64;
                let scale = math::attractor_scale(field) * a[7];
                let thickness = a[8];

                // Reseed (warm up onto the attractor, pre-fill the trail) when the
                // field / seed-count / seed-value changes — stateful otherwise.
                let key = (field, nseeds, seedval);
                if self.attr_key != key || self.attr_heads.len() != nseeds {
                    self.attr_heads.clear();
                    self.attr_trails.clear();
                    let base = math::attractor_seed_point(field);
                    for i in 0..nseeds {
                        // Deterministic seed-point offset in [-1,1]³ (xorshift hash).
                        let h = |k: u32| -> f64 {
                            let mut x = (seedval as u32)
                                .wrapping_mul(2_654_435_761)
                                ^ (i as u32).wrapping_add(k).wrapping_mul(40_503);
                            x ^= x << 13;
                            x ^= x >> 17;
                            x ^= x << 5;
                            (x as f64 / u32::MAX as f64) * 2.0 - 1.0
                        };
                        let mut p = base + DVec3::new(h(1), h(2), h(3)) * spread;
                        let mut dq = VecDeque::with_capacity(trail_len);
                        for n in 0..(300 + trail_len) {
                            p = math::attractor_rk4(field, p, dt);
                            if !p.is_finite() {
                                p = base;
                            }
                            if n >= 300 {
                                dq.push_back(p.as_vec3()); // raw; scaled at build
                            }
                        }
                        self.attr_heads.push(p);
                        self.attr_trails.push(dq);
                    }
                    self.attr_key = key;
                }

                // Advance: integrate `steps` sub-steps/frame (clock-driven), pushing
                // each to the trail ring so the streamline flows smoothly.
                let steps = if s.animate != 0 {
                    (s.rot_mod[3] as f64 * speed_mult * head_speed * 800.0)
                        .round()
                        .clamp(0.0, 256.0) as usize
                } else {
                    0
                };
                for si in 0..self.attr_heads.len() {
                    let mut p = self.attr_heads[si];
                    let tr = &mut self.attr_trails[si];
                    for _ in 0..steps {
                        p = math::attractor_rk4(field, p, dt);
                        if !p.is_finite() {
                            p = math::attractor_seed_point(field);
                        }
                        tr.push_back(p.as_vec3());
                    }
                    while tr.len() > trail_len {
                        tr.pop_front();
                    }
                    self.attr_heads[si] = p;
                }

                // Build strands (raw → display-scaled), tint swept along each trail.
                let cp = self.color_phase as f32;
                let tint_for = |t: f32| -> Vec4 {
                    let tc = t + cp;
                    if palette != 0 {
                        math::palette_tint(palette, tc)
                    } else {
                        math::hsv_tint(tc)
                    }
                };
                self.geom.gen_strands.restart();
                for tr in &self.attr_trails {
                    let n = tr.len();
                    if n < 1 {
                        continue;
                    }
                    let mut strand = Vec::with_capacity(n);
                    for (i, raw) in tr.iter().enumerate() {
                        let pos = *raw * scale;
                        let next = if i + 1 < n { tr[i + 1] * scale } else { pos };
                        let t01 = if n > 1 { i as f32 / (n as f32 - 1.0) } else { 0.0 };
                        strand.push(math::Frame {
                            position: pos,
                            tangent: Some((next - pos).normalize_or_zero()),
                            normal: None,
                            scale: Vec3::splat(thickness),
                            tint: tint_for(t01),
                        });
                    }
                    self.geom.gen_strands.push(strand);
                }
                // Streamlines: cube = beads, Flow/Swept-Tubes = flowing tubes,
                // Metaball reads the instances; Membrane falls back to tubes.
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::Harmonic => {
                let h = &s.harm;
                if s.bell[0] > 0.5 {
                    // Physical soft-body bell (#99): an XPBD sim that genuinely
                    // contracts + recoils, instead of the closed-form Σ Yₗᵐ sum.
                    // Reuses harm radius / θ-res / φ-res / thickness for the grid.
                    let nt = (h[10] as usize).clamp(3, 128);
                    let np = (h[11] as usize).clamp(3, 256);
                    let radius = h[9].max(1.0e-3);
                    let theta_max = s.bell[4];
                    let key = (nt, np, (radius * 64.0) as i32, (theta_max * 256.0) as i32);
                    if self.bell_key != key {
                        self.bell.rebuild(nt, np, radius, theta_max);
                        self.bell_key = key;
                        self.bell_accum = 0.0;
                    }
                    // Beat-paced contraction pump. The stroke phase advances *with the
                    // beat* (stays musical) at `stroke_rate` × 0.25 pulses/beat — slow
                    // and flowy by default (one pulse per bar), sped up via the param.
                    // `jelly_stroke` is a smooth C¹ squeeze→slower-recovery profile, so
                    // the membrane eases through it instead of snapping. Decoupled from
                    // the global Speed dial so the bell always breathes.
                    let stroke_rate = s.bell[5].max(0.0) as f64;
                    if s.animate != 0 {
                        self.bell_phase += dt_beats * stroke_rate * 0.25;
                    }
                    let stroke = jelly_stroke(self.bell_phase.rem_euclid(1.0) as f32);
                    let bp = math::BellParams {
                        stroke,
                        stroke_depth: s.bell[1],
                        iters: s.bell[2] as usize,
                        damping: s.bell[3],
                        volume: 0.5,
                    };
                    // Real-time fixed-dt substeps (XPBD wants a stable dt) — smooth +
                    // frame-rate independent. The slow stroke means the soft body eases
                    // through it; no responsiveness gain (that made it twitchy).
                    const SIM_DT: f64 = 1.0 / 120.0;
                    const MAX_SUBSTEPS: u32 = 8;
                    if s.animate != 0 {
                        self.bell_accum += dt;
                    }
                    let mut steps = 0;
                    while self.bell_accum >= SIM_DT && steps < MAX_SUBSTEPS {
                        self.bell.step(SIM_DT as f32, &bp);
                        self.bell_accum -= SIM_DT;
                        steps += 1;
                    }
                    if self.bell_accum > SIM_DT * MAX_SUBSTEPS as f64 {
                        self.bell_accum = 0.0;
                    }
                    self.bell.emit(h[12], palette, self.color_phase as f32, &mut self.geom.gen_strands);
                } else {
                    let modes = [
                        (h[0] as u32, h[1], h[2]),
                        (h[3] as u32, h[4], h[5]),
                        (h[6] as u32, h[7], h[8]),
                    ];
                    math::harmonic_strands(
                        modes,
                        h[9],           // radius
                        h[10] as usize, // θ resolution
                        h[11] as usize, // φ resolution
                        h[12],          // thickness
                        self.gen_phase as f32,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                }
                // Grid (meridians): Cube = beads on the bell, Swept Tubes = meridian
                // wireframe, Metaball = the smooth pulsing bell. Membrane lofts the
                // bell surface itself — closing the φ seam by repeating meridian 0.
                if membrane_mode && !membrane_arms {
                    let mut mem = math::strands_to_mem(&self.geom.gen_strands);
                    if mem.len() >= 2 {
                        let first = mem[0].clone();
                        mem.push(first); // wrap φ so the bell has no seam
                    }
                    let gx = mem.len();
                    math::loft_membrane(
                        &mem, gx, 1, 1, true, 0, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::LSystem => {
                let l = &s.ls;
                math::lsystem_strands(
                    l[0] as u32, // system
                    l[1] as u32, // depth
                    l[2],        // angle (deg)
                    l[3],        // step
                    l[4],        // sway amp
                    l[5],        // sway freq
                    l[6],        // grow
                    l[7],        // thickness
                    self.gen_phase as f32,
                    palette,
                    self.color_phase as f32,
                    CUBE_CEILING,
                    &mut self.geom.gen_strands,
                );
                // Tree topology: Cube = beads, Flow/Swept-Tubes = branches as tubes
                // (the canonical plant), Metaball = a blobby organism; Membrane
                // degrades to tubes (not parallel strands).
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::CurlNoise => {
                let c = &s.cn;
                // Flow speed evolves the noise in time off the global clock.
                let t = self.gen_phase as f32 * c[6];
                math::curlnoise_strands(
                    c[0] as usize,       // seeds
                    c[1] as u32,         // seed value
                    c[2],                // spread
                    c[3],                // field scale
                    c[4] as usize,       // steps
                    c[5],                // dt
                    t,                   // evolving noise time
                    c[7],                // containment
                    c[8],                // thickness
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                // Streamlines: Cube = particles, Flow/Swept-Tubes = smoke ribbons,
                // Metaball = an ink blob; Membrane degrades to tubes.
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::Polarization => {
                let p = &s.pol;
                let rings = (p[0] as usize).max(1);
                let spokes = (p[1] as usize).max(1);
                math::polarization_strands(
                    rings,
                    spokes,
                    p[2] as usize, // samples / ray
                    p[3],          // ray length R
                    p[4],          // wavenumber k
                    p[5],          // amplitude
                    p[6],          // falloff (1/r)
                    p[7] > 0.5,    // left-handed
                    p[8],          // spread (cone half-angle °)
                    p[9],          // swirl precession
                    p[10] > 0.5,   // show B helix
                    p[11],         // thickness
                    self.gen_phase as f32,
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                // Grid lattice of E rays (B follows when shown). Cube = beads on the
                // helices, Swept Tubes = glassy filaments, Metaball = a plasma core.
                // Membrane lofts a rippling shell across the ray fan — the E sheet
                // only (first rings·spokes strands), wrapping φ so each ring closes;
                // weaving across rings too (Web) gives the radiating "eye" net. B (if
                // on) still draws as tubes underneath.
                if membrane_mode && !membrane_arms && self.geom.gen_strands.len() >= rings * spokes && rings * spokes >= 2 {
                    let base = math::strands_to_mem(&self.geom.gen_strands);
                    let mut mem = Vec::with_capacity(rings * (spokes + 1));
                    for ri in 0..rings {
                        for si in 0..spokes {
                            mem.push(base[ri * spokes + si].clone());
                        }
                        mem.push(base[ri * spokes].clone()); // wrap φ → closed ring
                    }
                    math::loft_membrane(
                        &mem, rings, spokes + 1, 1, true, 4, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::MaxwellField => {
                let m = &s.maxwell;
                let lines = m[0] > 0.5;
                // #feat/maxwell-eb-blend: generator E↔B blend ∈ [0,1] (0 = E, 1 = B).
                let gen_blend = m[1];
                // Lattice displacement look: false = raw field magnitude (flowing
                // waves — the original), true = unit direction (uniform spokes). m[21].
                let field_norm = m[21] > 0.5;
                let count = (m[2] as usize).max(1);
                let dipoles = m[3] > 0.5;
                let phase = self.gen_phase as f32;
                // The field-oscillation clock ("ωt" in the retarded phase): the shared
                // Duo-Field `maxdip_phase`. Default = the free-running audio-dipole clock
                // (#248 Tier 3: pitch-scaled while the drive is on; hard-synced to gen_phase
                // when off → byte-identical pre-#248). **Tempo Sync** (m[22]) makes it an LFO
                // phase-locked to the PLL beat clock — one full field there-and-back per note
                // division — applied centrally where `maxdip_phase` is advanced, so the
                // geometry here, the aura/energy cloud, and the B swirl all share it. The
                // source layout swirl stays on the global clock (`phase`).
                let fphase = self.maxdip_phase as f32;
                // #248 Tier 1: the audio drive scales the displayed field vectors
                // (lattice mode's amp) — E scales linearly with the source drive, so
                // the arrows breathe with the music like the energy cloud does.
                let drive = audio_dipole_drive(&s);
                // #248 Tier 2: spectrum → multipole. The band envelopes drive a
                // stack of distinct multipole moments that REPLACES the point
                // sources (like the finite antenna does for energization) — the
                // geometry's shape becomes the spectrum. Band envelopes carry the
                // loudness, so the Tier-1 RMS drive doesn't double-apply here.
                let band_elems = if audio_multipole_on(&s) {
                    audio_band_elems(&s)
                } else {
                    Vec::new()
                };
                let band_mode = !band_elems.is_empty();
                // Sources laid out + orbited (swirl) off the global clock; the Tier-3
                // stereo lean shifts them with the mix balance.
                let mut sources = math::maxwell_sources(count, m[4], dipoles, m[5], m[6], phase);
                let lean = audio_stereo_lean(&s);
                if lean != 0.0 {
                    for src in &mut sources {
                        src.pos.x += lean;
                    }
                }
                if fv_lines_active {
                    // Volumetric field-lines (#348/#349, Maxwell parity): a dense cloud of
                    // thin glowing streamlines of BOTH channels — the E field (blend 0,
                    // warm) + the B field (blend 1, cool) — flowing over each other, the
                    // tube-mode structure as a volumetric filament cloud. Colours = the LUT
                    // warm/cool ends when Calibrated colour is on, else ember/indigo.
                    let cm = organon_core::params::ColourMode::from_u32(s.colour[0] as u32);
                    let (ca, cb) = if matches!(cm, organon_core::params::ColourMode::Calibrated) {
                        let lut = s.colour[3] as u32;
                        let e = math::calibrated_colour(0.85, lut);
                        let b = math::calibrated_colour(0.12, lut);
                        (Vec4::new(e.x, e.y, e.z, 1.0), Vec4::new(b.x, b.y, b.z, 1.0))
                    } else {
                        (Vec4::new(1.0, 0.42, 0.10, 1.0), Vec4::new(0.10, 0.52, 1.0, 1.0))
                    };
                    let density = (s.fieldvol[6] as usize).max(1);
                    let thickness = s.fieldvol[7].max(0.001);
                    let radius = (m[4].max(1.0) * 3.0).clamp(4.0, 22.0);
                    let step_ds = (radius / 60.0).max(0.02);
                    let flow_phase = self.beat_pos as f32 * std::f32::consts::TAU * 2.0;
                    // Build the two channel fields (E = blend 0, B = blend 1) — the
                    // band-multipole field when audio-multipole is on so the lines follow
                    // the spectrum-shaped field like the duo-volume + arrows do (Bugbot),
                    // else the point/dipole field with the finite antenna. Direction-only,
                    // so `drive` is irrelevant.
                    let antenna_segs = if s.maxenergy[5] != 0.0 { 64 } else { 0 };
                    let mk = |blend: f32| -> math::AnalyticField {
                        if band_mode {
                            math::AnalyticField::MaxwellBands {
                                elems: band_elems.clone(),
                                blend,
                                near: m[7],
                                r_min: m[10],
                                phase: fphase,
                            }
                        } else {
                            math::AnalyticField::Maxwell {
                                sources: sources.clone(),
                                dipoles,
                                blend,
                                k: m[8],
                                near: m[7],
                                r_min: m[10],
                                phase: fphase,
                                antenna_len: s.maxenergy[4],
                                antenna_segs,
                                drive: 1.0,
                                offset: Vec3::ZERO,
                            }
                        }
                    };
                    let field_e = mk(0.0);
                    let field_b = mk(1.0);
                    math::maxwell_lines_volumetric_strands(
                        &field_e, ca, &field_b, cb, thickness, density, 120, step_ds,
                        Vec3::ZERO, radius, flow_phase, &mut self.geom.gen_strands,
                    );
                    self.geom.emit_strands(true, caps, weld)
                } else if lines {
                    // Level 2: field-line streamlines (Streamlines topology). Swept
                    // Tubes = glassy field lines, Metaball = a plasma core; Membrane
                    // degrades to tubes.
                    if band_mode {
                        math::maxwell_band_lines_strands(
                            &band_elems,
                            gen_blend,
                            (m[17] as usize).max(1) * count, // seeds (density ≈ seeds/source × sources)
                            m[18] as usize,                  // max steps
                            m[19],                           // step ds
                            m[20],                           // bound
                            m[7],                            // near
                            m[10],                           // r_min
                            m[11],                           // thickness
                            fphase,
                            s.audiodip[5],                   // colour by band
                            s.maxenergy[3],                  // base ember hue
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    } else {
                        math::maxwell_lines_strands(
                            &sources, dipoles, gen_blend,
                            m[17] as usize, // seeds / source
                            m[18] as usize, // max steps
                            m[19],          // step ds
                            m[20],          // bound
                            m[8],           // k
                            m[7],           // near
                            m[10],          // r_min
                            m[11],          // thickness
                            fphase,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else {
                    // Level 1: the field-vector tip on a (θ,φ) lattice (Grid). One
                    // dipole reads as the radiation lobe; Membrane lofts that shell.
                    let rings = (m[12] as usize).max(1);
                    let spokes = (m[13] as usize).max(1);
                    if band_mode {
                        math::maxwell_band_lattice_strands(
                            rings, spokes,
                            m[14] as usize, // samples / ray
                            m[15],          // ray length
                            m[16],          // spread (cone half-angle °)
                            &band_elems, gen_blend, field_norm,
                            m[7],           // near
                            m[9],           // amp (bands carry the loudness)
                            m[10],          // r_min
                            m[11],          // thickness
                            fphase,
                            s.audiodip[5],  // colour by band
                            s.maxenergy[3], // base ember hue
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    } else {
                        math::maxwell_lattice_strands(
                            rings, spokes,
                            m[14] as usize, // samples / ray
                            m[15],          // ray length
                            m[16],          // spread (cone half-angle °)
                            &sources, dipoles, gen_blend, field_norm,
                            m[8],          // k
                            m[7],          // near
                            m[9] * drive,  // amp × the audio drive (#248 Tier 1)
                            m[10],         // r_min
                            m[11], // thickness
                            fphase,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    }
                    if membrane_mode && !membrane_arms && self.geom.gen_strands.len() >= rings * spokes && rings * spokes >= 2 {
                        let base = math::strands_to_mem(&self.geom.gen_strands);
                        let mut mem = Vec::with_capacity(rings * (spokes + 1));
                        for ri in 0..rings {
                            for si in 0..spokes {
                                mem.push(base[ri * spokes + si].clone());
                            }
                            mem.push(base[ri * spokes].clone()); // wrap φ → closed ring
                        }
                        math::loft_membrane(
                            &mem, rings, spokes + 1, 1, true, 4, membrane_close, palette, self.color_phase as f32,
                            &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                        );
                        draw_membrane_mesh = true;
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                }
            }
            GeneratorMode::Acoustic => {
                let a = &s.acoustic;
                let a2 = &s.acoustic2;
                let rings = (a[8] as usize).max(1);
                let spokes = (a[9] as usize).max(1);
                // #325 Tier 3: the pitch-scaled field clock (shared audio-dipole
                // clock; hard-synced to gen_phase when the drive is off →
                // byte-identical pre-audio).
                let phase = self.maxdip_phase as f32;
                if fv_lines_active {
                    // Volumetric field-lines: trace a dense set of thin streamlines of BOTH
                    // channels (compression + transverse) from volume-filling seeds → a
                    // glowing filament cloud (the tube-mode flow, without chunky tubes).
                    let band_drives = audio_band_drives(&s);
                    let use_bands = audio_multipole_on(&s) && band_drives.iter().any(|d| *d > 0.0);
                    let sources = if use_bands {
                        math::acoustic_band_sources(&band_drives, a[4])
                    } else {
                        math::acoustic_sources(math::AcousticKind::from_u32(a[0] as u32), a[4])
                    };
                    // Channel colours: the LUT's warm/cool ends when Calibrated colour is on,
                    // else the ember/indigo Duo-Field native tints.
                    let cm = organon_core::params::ColourMode::from_u32(s.colour[0] as u32);
                    let (ca, cb) = if matches!(cm, organon_core::params::ColourMode::Calibrated) {
                        let lut = s.colour[3] as u32;
                        let p = math::calibrated_colour(0.85, lut);
                        let v = math::calibrated_colour(0.12, lut);
                        (Vec4::new(p.x, p.y, p.z, 1.0), Vec4::new(v.x, v.y, v.z, 1.0))
                    } else {
                        (Vec4::new(1.0, 0.42, 0.10, 1.0), Vec4::new(0.10, 0.52, 1.0, 1.0))
                    };
                    let density = (s.fieldvol[6] as usize).max(1);
                    let thickness = s.fieldvol[7].max(0.001);
                    let radius = (a[4].max(1.0) * 3.0).clamp(4.0, 22.0);
                    let step_ds = (radius / 60.0).max(0.02);
                    // Flow pulse rides the beat clock (2 cycles/beat) so the filaments
                    // stream in time with the music (and keep flowing when it's stopped —
                    // the beat clock free-runs at the manual tempo).
                    let flow_phase = self.beat_pos as f32 * std::f32::consts::TAU * 2.0;
                    math::acoustic_lines_strands(
                        &sources, a[1], a[2], a[5], phase, thickness, density, 120, step_ds,
                        Vec3::ZERO, radius, ca, cb, flow_phase, &mut self.geom.gen_strands,
                    );
                    self.geom.emit_strands(true, caps, weld) // force swept tubes (glowing filaments)
                } else {
                if a2[0] > 0.5 {
                    // #325 Tier 4: cavity standing-wave (Chladni) — a bounded room
                    // eigenmode whose pressure nodal planes are the visible-node
                    // showpiece; `cav_morph` walks the modes on the beat. Tier 5: the
                    // walk is tweened (holds then glides between mode sets, `acoustic3[0]`)
                    // + breathes in 3-D with the audio (per-axis gains `acoustic3[1..4]`).
                    let a3 = &s.acoustic3;
                    let base_modes = Vec3::new(a2[1], a2[2], a2[3]);
                    let mut modes =
                        math::cavity_morph_modes_tween(base_modes, self.beat_pos, a2[4], a3[0]);
                    modes += Vec3::new(a3[1], a3[2], a3[3]) * cavity_audio_breathe(&s);
                    let dims = Vec3::splat(a2[5].max(1.0e-3));
                    // #325 Tier 5: audio drives the source in Cavity too (was radiating-
                    // only) — the RMS drive × beat pump swells the whole standing wave.
                    let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                    let amp = a[3] * audio_dipole_drive(&s) * pump;
                    math::acoustic_cavity_lattice_strands(
                        rings,
                        spokes,
                        a[10] as usize, // samples / ray
                        a[11],          // ray length
                        a[12],          // spread (cone half-angle °)
                        modes,
                        dims,
                        a[6],       // geometry compression↔transverse blend
                        a[7] > 0.5, // unit-field (else raw) displacement
                        amp,        // amplitude × audio drive × beat pump
                        a[13],      // thickness
                        phase,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                } else {
                    let kind = math::AcousticKind::from_u32(a[0] as u32);
                    // Beat pump (#325 Tier 3): the pulse envelope swells the source amp.
                    let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                    // Spectrum → multipole: a band-weighted monopole stack replaces the
                    // fixed source (the spectrum becomes the field's spatial structure).
                    // Fall back to the static layout when every band is silent (drive floor
                    // 0 + quiet audio), else the field goes dark on a zero-strength placeholder.
                    let band_drives = audio_band_drives(&s);
                    let use_bands = audio_multipole_on(&s) && band_drives.iter().any(|d| *d > 0.0);
                    let mut sources = if use_bands {
                        math::acoustic_band_sources(&band_drives, a[4])
                    } else {
                        math::acoustic_sources(kind, a[4])
                    };
                    // In genuine band mode the loudness is already baked into each source's
                    // q, so don't scale by the broadband RMS drive again (Maxwell band mode
                    // omits it too — else energy ~drive² on top of band amplitudes). On the
                    // static-layout fallback, apply the normal broadband drive.
                    let drive = if use_bands { 1.0 } else { audio_dipole_drive(&s) };
                    // Stereo → source position (#325 Tier 3): lean the stack along X.
                    let lean = if s.audiodip[0] != 0.0 {
                        s.audio[7].clamp(-1.0, 1.0) * s.audiodip[6].clamp(0.0, 1.0) * a[4].max(1.0)
                    } else {
                        0.0
                    };
                    if lean != 0.0 {
                        for src in &mut sources {
                            src.pos.x += lean;
                        }
                    }
                    math::acoustic_lattice_strands(
                        rings,
                        spokes,
                        a[10] as usize,      // samples / ray
                        a[11],               // ray length
                        a[12],               // spread (cone half-angle °)
                        &sources,
                        a[6],                // geometry compression↔transverse blend
                        a[7] > 0.5,          // unit-field (else raw) displacement
                        a[1],                // k
                        a[2],                // circulation strength
                        a[3] * drive * pump, // amp × audio drive × beat pump
                        a[5],                // r_min
                        a[13],               // thickness
                        phase,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                }
                // Membrane. Two forms:
                //  • Shell (default): loft the closed azimuthal shell (first
                //    rings·spokes strands), wrapping φ so each ring closes — the
                //    radiating / cavity sound shell.
                //  • Skin Arms: skin each strand (arm) as its own closed finger with
                //    open gaps between arms — the hull of the volume-render form. Built
                //    as a welded Mesh (seamless swept tubes) or capsule Impostors (the
                //    strands lower to per-segment rods below; the impostor pass reuses
                //    them). Either way the shell sheet is NOT drawn.
                // Skin Arms is handled generically post-match (each strand → a finger);
                // here we only build the shell when arms are off.
                if membrane_mode && !membrane_arms
                    && self.geom.gen_strands.len() >= rings * spokes && rings * spokes >= 2
                {
                    let base = math::strands_to_mem(&self.geom.gen_strands);
                    let mut mem = Vec::with_capacity(rings * (spokes + 1));
                    for ri in 0..rings {
                        for si in 0..spokes {
                            mem.push(base[ri * spokes + si].clone());
                        }
                        mem.push(base[ri * spokes].clone()); // wrap φ → closed ring
                    }
                    math::loft_membrane(
                        &mem, rings, spokes + 1, 1, true, 4, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
                }
            }
            GeneratorMode::MapAttractor => {
                // #380: iterate the discrete complex-holomorphic map for many points →
                // a node cloud (fills instances/tints, so every surface mode works —
                // SurfaceMode::Splat gives the additive density "fire").
                // Tier 2: the effective (a,b) come from the beat-locked PARAMETER ORBIT
                // (`maporbit[8]`), computed by the shared `map_attractor_effective_ab`
                // (Off = static; Linear = the Tier-1 `a += a_drive·gen_phase` ramp,
                // byte-identical with the drives 0; Lissajous = a closed loop walked at
                // `beat_pos / loop_beats`, free-running on `gen_phase` when stopped).
                let m = &s.mapattractor;
                let o = &s.maporbit;
                // Tier 3: [c, d, color, _] — extra map coefficients + colour-by-dynamics.
                let m2 = &s.mapattractor2;
                self.geom.gen_strands.restart();
                let playing = s.transport[0] > 0.5;
                let ab = math::map_attractor_effective_ab(
                    m, o, self.beat_pos, self.gen_phase, playing,
                );
                let opts = math::MapAttractorOpts {
                    kind: math::MapKind::from_u32(m[0] as u32),
                    a: ab.x,
                    b: ab.y,
                    c: m2[0],
                    d: m2[1],
                    // Clamp to the param range (points_k 1..400 → 1..400_000, warmup 0..2000):
                    // `f32 as usize` saturates, so a corrupted/out-of-range IPC value would
                    // otherwise force an enormous allocation + per-frame trace (cf. the Field
                    // Engine seed clamp).
                    points: ((m[3].max(0.0) * 1000.0) as usize).clamp(1, 400_000),
                    warmup: (m[4].max(0.0) as usize).min(2000),
                };
                math::map_attractor_field(
                    &opts,
                    math::MapColor::from_u32(m2[2] as u32),
                    m[5],     // world scale (half-extent of the [-1,1] box)
                    m[6],     // per-point marker size
                    m[7],     // intensity (tint gain)
                    palette,
                    &mut self.map_points,
                    &mut self.geom.instances,
                    &mut self.geom.tints,
                )
            }
            GeneratorMode::Phyllotaxis => {
                let p = &s.phyl;
                let m = (p[4] as usize).max(1);
                math::phyllotaxis_strands(
                    p[0] as u32,   // surface
                    p[1] as usize, // count
                    p[2],          // divergence °
                    p[3],          // radius scale
                    m,             // parastichy
                    p[5],          // height
                    p[6],          // shell growth
                    p[7],          // breathe amp
                    p[8],          // breathe freq
                    self.gen_phase as f32, // rotation + breathing clock
                    p[9],          // rotation speed
                    p[10],         // thickness
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                // Grid of m parastichy spirals → Membrane skins a ribbon between
                // adjacent arms (wrap the spiral family so the sheet closes).
                if membrane_mode && !membrane_arms && self.geom.gen_strands.len() >= m && m >= 2 {
                    let base = math::strands_to_mem(&self.geom.gen_strands);
                    let mut mem = Vec::with_capacity(m + 1);
                    for sp in 0..m {
                        mem.push(base[sp].clone());
                    }
                    mem.push(base[0].clone()); // wrap the parastichy family
                    math::loft_membrane(
                        &mem, m + 1, 1, 1, true, 1, membrane_close, palette, self.color_phase as f32,
                        &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::Boids => {
                let b = &s.boids;
                let count = (b[0] as usize).clamp(1, 4096);
                let seed = b[12] as u32;
                let trail = (b[8] as usize).clamp(2, 512);
                let bounds = b[9].max(0.1);
                let scale = b[14].max(0.001);
                let thickness = b[11];
                let sim_speed = b[13].max(0.0) as f64;

                // Reseed when the structural key changes (count / seed / trail).
                // Spawn inside ~80% of the cage so the soft bound isn't fighting
                // it at t=0.
                let key = (count, seed, trail);
                if self.boids_key != key {
                    self.boids.reseed(count, seed, trail, bounds * 0.8);
                    self.boids_key = key;
                    self.boids_accum = 0.0;
                }

                // Beat-pulsed goal pull: gather on the downbeat, scatter between
                // (only while Pulse is on; otherwise a steady gentle centring).
                let goal_w = b[10] * if s.pulse != 0 { pulse_env } else { 1.0 };
                let bp = math::BoidsParams {
                    perception: b[1].max(1e-3),
                    separation: b[2].max(1e-3),
                    sep_weight: b[3],
                    align_weight: b[4],
                    cohere_weight: b[5],
                    max_speed: b[6].max(1e-3),
                    max_force: b[7].max(1e-3),
                    bounds,
                    goal_weight: goal_w,
                };

                // Fixed-dt accumulator: advance in stable SIM_DT chunks; the
                // wall-clock rate rides the global Speed dial (× decade/pulse) ×
                // the Boids sim-speed param, so the flock speeds up / slows + rides
                // the beat. Capped per frame to avoid a spiral of death after a stall.
                const SIM_DT: f64 = 1.0 / 120.0;
                const SPEED_GAIN: f64 = 240.0;
                const MAX_SUBSTEPS: u32 = 6;
                if s.animate != 0 {
                    let rate = s.rot_mod[3] as f64 * speed_mult * sim_speed * SPEED_GAIN;
                    self.boids_accum += dt * rate;
                }
                let mut iters = 0;
                while self.boids_accum >= SIM_DT && iters < MAX_SUBSTEPS {
                    self.boids.step(SIM_DT as f32, &bp);
                    self.boids_accum -= SIM_DT;
                    iters += 1;
                }
                // Drop any backlog beyond the cap so we don't fast-forward later.
                if self.boids_accum > SIM_DT * MAX_SUBSTEPS as f64 {
                    self.boids_accum = 0.0;
                }

                // Creature form (#52): when set, draw one fish/bird/… per agent
                // (oriented by velocity) instead of the surface mode. Otherwise emit
                // the trails as Streamline strands and lower them like the attractor
                // (cube = beads, Flow/Swept-Tubes = flowing tubes, Metaball reads the
                // instances, Membrane falls back to tubes).
                match BoidsForm::from_u32(b[15] as u32).creature_kind() {
                    Some(kind) => {
                        boids_creature = kind as i32;
                        let size = b[16].max(0.01);
                        let bank = b[17];
                        self.boids.emit_creatures(
                            scale,
                            size,
                            bank,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.instances,
                            &mut self.geom.tints,
                        )
                    }
                    None => {
                        self.boids.emit(
                            scale,
                            thickness,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                        let fa = flow_aligned || strand_membrane_fallback;
                        self.geom.emit_strands(fa, caps, weld)
                    }
                }
            }
            GeneratorMode::Tessellation => {
                // Aperiodic tiling (#121: Penrose P3). The `view` param is the
                // generator's own 2D→3D ladder (it overrides the surface mode):
                //   Edges (0)    → tile edges as swept rods (Phase 1);
                //   Filled (1)   → flat triangulated tiles (membrane mesh);
                //   Extruded (2) → per-tile prisms, the "cityscape" (membrane mesh).
                // Filled/Extruded ride the membrane mesh path, so PBR/Chrome/Glass
                // apply. Phase 3 beat motion: a gentle swell on the beat (beat_infl)
                // + a radial ripple (locked to the beat clock) that lifts/glows each
                // tile. (The whole scene already rides global Breath via the view
                // uniform — `build_uniforms` scales by `breath_scale` — so we do NOT
                // multiply it here; doing so double-applied Breath and ballooned the
                // tiling even at beat_infl 0.)
                // Phase 4: `construct` (t[10]) picks inflation vs cut-and-project
                // (de Bruijn multigrid). Cut-and-project unlocks Ammann–Beenker
                // (forced for that family) + phason flips — advancing the window
                // phase (t[11]=amount) by the gen clock continuously rearranges the
                // tiling. `grid_n` (t[12]) is the multigrid tile-count dial.
                let t = &s.tessellation;
                let family = t[0] as u32;
                let depth = t[1].max(0.0) as usize;
                let construct = t[10] as u32;
                let grid_n = t[12].max(1.0) as usize;
                let phason_amt = t[11];
                // The window orbits with the global animation clock (rides Speed).
                let phason_phase = self.gen_phase as f32;
                let beat = if s.pulse != 0 { pulse_env } else { 0.0 };
                let infl = 1.0 + t[7] * 0.2 * beat; // gentle beat swell (≤ +20% at amount 1)
                let scale = t[2] * infl;
                let view = t[4] as u32;
                if view == 3 {
                    // Honest 3-D icosahedral quasicrystal (Z⁶ cut-and-project rod
                    // lattice) — its own 3-D structure (family ignored); phason
                    // animates the perp-space window.
                    math::quasicrystal3d_strands(
                        grid_n, phason_phase, phason_amt, scale, t[3], palette,
                        self.color_phase as f32, &mut self.geom.gen_strands,
                    );
                    self.geom.emit_strands(true, caps, weld)
                } else if family == 4 {
                    // Truchet: arcs that link into labyrinth curves → always rods
                    // (the family is inherently curves; ignores the fill view).
                    math::truchet_strands(
                        grid_n, scale, t[3], palette, self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                    self.geom.emit_strands(true, caps, weld)
                } else if family == 5 {
                    // Hyperbolic {p,q} Circle-Limit (p = t[14], q = t[15]) → geodesic
                    // arc rods in the Poincaré disk. Its own curved geometry (ignores
                    // the fill view).
                    math::hyperbolic_strands(
                        t[14] as usize, t[15] as usize, depth, scale, t[3], palette,
                        self.color_phase as f32, &mut self.geom.gen_strands,
                    );
                    self.geom.emit_strands(true, caps, weld)
                } else if view == 0 {
                    // Edges: 1-D segments → always flow-aligned (rods).
                    math::tessellation_strands(
                        family, construct, depth, grid_n, phason_phase, phason_amt,
                        scale, t[3], palette, self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                    // Ammann bars (#121 follow-up): overlay the de Bruijn grid lines
                    // on the cut-and-project Edges view (where the grid is defined).
                    // Pinwheel (family 3) always uses its own substitution, never the
                    // multigrid, so the grid-line bars don't apply to it — exclude it.
                    if t[13] > 0.0 && family != 3 && math::tess_use_multigrid(family, construct) {
                        math::ammann_bars(
                            family, grid_n, phason_phase, phason_amt, scale, t[3],
                            palette, self.color_phase as f32, &mut self.geom.gen_strands,
                        );
                    }
                    self.geom.emit_strands(true, caps, weld)
                } else {
                    // Filled / extruded tiles → a world-space membrane mesh. Clear
                    // the instanced buffers so only the mesh draws. The ripple wave
                    // sweeps with the continuous beat position (t[8]=amt, t[9]=freq).
                    self.geom.instances.clear();
                    self.geom.tints.clear();
                    let b = math::tessellation_mesh(
                        family, construct, depth, grid_n, phason_phase, phason_amt,
                        scale, view, t[5], t[6] as u32,
                        self.beat_pos as f32, t[8], t[9], palette,
                        self.color_phase as f32, &mut self.mem_pos, &mut self.mem_norm,
                        &mut self.mem_col, &mut self.mem_idx,
                    );
                    draw_membrane_mesh = true;
                    b
                }
            }
            GeneratorMode::Mandelbulb => {
                // Raymarched fractal — no nodes. Clear the instanced buffers so
                // nothing else draws; the bulb is rendered by its own raymarch
                // path below. Report bounds so the camera frames it (centre 0,
                // radius = world scale, breathing with the pulse).
                self.geom.instances.clear();
                self.geom.tints.clear();
                draw_membrane_mesh = false;
                let scale = s.mandelbulb[2].max(1e-3) * breath_scale.x;
                let r = Vec3::splat(scale * 1.3);
                math::Bounds { min: -r, max: r }
            }
            GeneratorMode::Creature => {
                // Raymarched SDF creature (#476) — no nodes. Clear the instanced
                // buffers; the creature is drawn by its own raymarch path below.
                // Report bounds so the camera frames it (centre 0, radius = the
                // body plan's bound × world scale, breathing with the pulse).
                self.geom.instances.clear();
                self.geom.tints.clear();
                draw_membrane_mesh = false;
                let form = s.creature[0].max(0.0) as u32;
                let scale = s.creature[1].max(1e-3) * breath_scale.x;
                let bound = math::creature_bounds(&math::creature_body_plan(form));
                let r = Vec3::splat(scale * bound * 1.1);
                math::Bounds { min: -r, max: r }
            }
            GeneratorMode::Kaleidoscope => {
                // No nodes — the fullscreen flat/tunnel field is painted by its own
                // pass below and is camera-independent (unit bounds).
                self.geom.instances.clear();
                self.geom.tints.clear();
                draw_membrane_mesh = false;
                let r = Vec3::splat(1.0);
                math::Bounds { min: -r, max: r }
            }
            GeneratorMode::MinimalSurface => {
                let m = &s.minimal_surface;
                let family = m[0] as u32;
                if math::minimal_is_parametric(family) {
                    // Parametric: a (u,v) Grid of frames lowered to instances + skinned
                    // by the membrane loft like the harmonic bell, so every surface mode
                    // + material applies. `closed` = the u-seam wraps (a tube).
                    let res = (m[10] as usize).clamp(8, 256);
                    let closed;
                    if family == math::MINIMAL_UNDULOID || family == math::MINIMAL_NODOID {
                        // CMC surface of revolution (Phase 4b). neck = static bend phase
                        // (m[12]); bend speed (m[9]) pulses it = mean-curvature flow (the
                        // bulges breathe). repeats = turns (m[13]); twist (m[5]).
                        let neck = (m[12] + 0.3 * m[9] * (self.gen_phase as f32).sin())
                            .clamp(0.0, 1.0);
                        math::minimal_cmc_strands(
                            family == math::MINIMAL_NODOID,
                            m[1].max(1.0e-3), // scale (breath rides the view matrix)
                            res,
                            res,
                            neck,
                            m[13].max(0.5),   // repeats (turns slot) = bulge count
                            m[5],             // twist (helical warp)
                            m[4].max(1.0e-3), // node thickness
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                        closed = true; // a revolution tube is always u-periodic
                    } else {
                        // Weierstrass–Enneper (Phase 2): θ = home + static bend position
                        // (m[12]·π) + animated bend (m[9] rides the global clock).
                        let base = match family {
                            x if x == math::MINIMAL_CATENOID => std::f32::consts::FRAC_PI_2,
                            _ => 0.0, // helicoid home; enneper ignores theta
                        };
                        let theta = base
                            + m[12] * std::f32::consts::PI
                            + self.gen_phase as f32 * m[9];
                        math::minimal_param_strands(
                            family,
                            m[1].max(1.0e-3),
                            res,
                            res,
                            m[11],            // extent
                            m[13],            // turns (u-domain revolutions)
                            theta,
                            m[5],             // twist
                            m[4].max(1.0e-3), // node thickness
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                        closed = math::minimal_u_closed(family, theta, m[13]);
                    }
                    if membrane_mode && !membrane_arms {
                        let mut mem = math::strands_to_mem(&self.geom.gen_strands);
                        // Close the u seam for a periodic surface (catenoid / CMC tube)
                        // by repeating the first u-row — like the bell wraps φ.
                        if closed && mem.len() >= 2 {
                            let first = mem[0].clone();
                            mem.push(first);
                        }
                        let gx = mem.len();
                        math::loft_membrane(
                            &mem, gx, 1, 1, false, 0, membrane_close, palette, self.color_phase as f32,
                            &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col,
                            &mut self.mem_idx,
                        );
                        draw_membrane_mesh = true;
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else {
                    // Implicit (Phase 1): raymarched TPMS isosurface — no nodes. Clear
                    // the instanced buffers; the surface is rendered by its own
                    // raymarch path below. Report bounds so the camera frames it
                    // (centre 0, radius = world scale, breathing with the pulse).
                    self.geom.instances.clear();
                    self.geom.tints.clear();
                    draw_membrane_mesh = false;
                    let scale = m[1].max(1e-3) * breath_scale.x;
                    let r = Vec3::splat(scale * 1.3);
                    math::Bounds { min: -r, max: r }
                }
            }
            GeneratorMode::Synchrotron => {
                // Liénard–Wiechert field of orbiting charge(s) (#150). Two views
                // (slot [10]): field arrows on a plane (Phase 1) or traced E field
                // lines (Phase 3). Both Streamlines; the pattern rotates as gen_phase
                // (observer time t) advances the orbit, so it rides Speed + the beat.
                // Swept Tubes / Flow-Aligned = glassy arrows / field lines; Membrane
                // degrades to tubes via the strand fallback.
                let y = &s.synchrotron;
                match y[10] as u32 {
                    1 => {
                        // Field-line view (Phase 3): RK4 streamlines of E.
                        math::synchrotron_lines_strands(
                            y[0],           // orbit radius R
                            y[1],           // beta = v/c
                            y[2] as usize,  // bunched charges
                            y[11] as usize, // line seeds
                            y[12] as usize, // line max steps
                            y[13],          // line step ds
                            y[14],          // line bound
                            y[5],           // near-field weight
                            y[8],           // source clamp r_min
                            y[7],           // thickness
                            y[19].to_radians(), // orbit-plane tilt
                            y[20],          // precession rate
                            self.gen_phase as f32,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    }
                    2 => {
                        // Field-volume view (Phase 4): the arrow plane extruded into a box.
                        // Reveal (cull dead field) + sphere inversion are P5 legibility toggles.
                        math::synchrotron_volume_strands(
                            y[0],           // orbit radius R
                            y[1],           // beta = v/c
                            y[2] as usize,  // bunched charges
                            y[3] as usize,  // grid (in-plane samples/axis)
                            y[15] as usize, // volume depth layers
                            y[4],           // half-extent (in-plane + depth)
                            y[5],           // near-field weight
                            y[6],           // arrow gain
                            y[7],           // thickness
                            y[8],           // source clamp r_min
                            y[16],          // reveal threshold (cull weak field)
                            y[17] > 0.5,    // sphere inversion (inside-out)
                            y[18],          // inversion radius
                            y[19].to_radians(), // orbit-plane tilt
                            y[20],          // precession rate
                            self.gen_phase as f32,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    }
                    _ => {
                        // Field-arrow view (Phase 1): a single plane.
                        math::synchrotron_strands(
                            y[0],          // orbit radius R
                            y[1],          // beta = v/c
                            y[2] as usize, // bunched charges
                            y[3] as usize, // grid (samples/axis)
                            y[4],          // plane half-extent
                            y[5],          // near-field weight (0 = radiation only)
                            y[6],          // arrow gain
                            y[7],          // thickness
                            y[8],          // source clamp r_min
                            y[9] > 0.5,    // sample the perpendicular plane
                            y[16],         // reveal threshold (cull weak field)
                            y[19].to_radians(), // orbit-plane tilt
                            y[20],         // precession rate
                            self.gen_phase as f32,
                            palette,
                            self.color_phase as f32,
                            &mut self.geom.gen_strands,
                        );
                    }
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::VectorField => {
                // Vector-field plotter (#173): sample a bank function
                // F(x, y, z) on a lattice as arrows (Tier 1), RK4-trace its
                // field lines (Tier 2, slot [13] view), or both (faint arrows
                // under the lines). gen_phase drives the domain-rotation
                // `evolve` + the flow pulse, so the field rides Speed + the
                // beat. Streamlines; Swept Tubes / Flow-Aligned = the reel's
                // look; Membrane degrades to tubes via the fallback.
                let v = &s.vecfield;
                let view = v[13] as u32;
                // Tier 3: the function-builder spec (only consulted when the
                // bank entry is Custom — the decode is control-rate cheap).
                let built = math::VecBuildSpec::from_slots(&s.vecbuild);
                let spec = (v[0] as u32 == math::VECFIELD_CUSTOM).then_some(&built);
                if view == 3 {
                    // Stream surface: equal-length field lines from an ordered
                    // seed curve → Grid topology. Membrane lofts the sheet
                    // (like Frenet/DNA); every other surface mode reads the
                    // same strands as evenly-cut field lines.
                    math::vecfield_surface_strands(
                        v[0] as u32,    // function bank preset
                        spec,           // Tier-3 builder (Custom entry)
                        v[14] as u32,   // seed curve (ring / line; from seeding)
                        v[15] as usize, // lines across the sheet
                        v[16] as usize, // steps per line (fixed length)
                        v[17],          // step length ds
                        v[18] > 0.5,    // bidirectional tracing
                        v[4],           // box half-extent
                        v[5],           // domain scale k
                        v[19] as u32,   // line colour (|F| / sweep)
                        v[22],          // line thickness
                        v[20],          // flow-pulse amount
                        v[21],          // flow-pulse speed
                        v[10],          // evolve (same rotating domain)
                        v[11],          // z lift (same lifted field)
                        self.gen_phase as f32,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                    if membrane_mode && !membrane_arms {
                        let mem = math::strands_to_mem(&self.geom.gen_strands);
                        let gx = mem.len();
                        math::loft_membrane(
                            &mem, gx, 1, 1, true, 0, membrane_close, palette, self.color_phase as f32,
                            &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col,
                            &mut self.mem_idx,
                        );
                        draw_membrane_mesh = true;
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else {
                if view != 1 {
                    // Arrow lattice (Tier 1) — alone, or dimmed under the lines.
                    math::vecfield_strands(
                        v[0] as u32,    // function bank preset
                        spec,           // Tier-3 builder (Custom entry)
                        v[1] as usize,  // grid x (1 = plane)
                        v[2] as usize,  // grid y
                        v[3] as usize,  // grid z (1 = the 2-D plot)
                        v[4],           // box half-extent
                        v[5],           // domain scale k
                        v[6],           // arrow gain
                        v[7],           // thickness
                        v[8] as u32,    // |F| → length map (soft/log/uniform)
                        v[9] as u32,    // tint mode (magnitude/direction)
                        v[10],          // evolve (domain-rotation speed)
                        v[11],          // z lift (planar presets → 3-D)
                        v[12],          // reveal threshold (cull weak field)
                        self.gen_phase as f32,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                    if view == 2 {
                        math::dim_strands(&mut self.geom.gen_strands, 0.35);
                    }
                } else {
                    self.geom.gen_strands.restart();
                }
                if view != 0 {
                    // Field lines (Tier 2) — appended over the (dimmed) arrows.
                    math::vecfield_lines_append(
                        v[0] as u32,    // function bank preset
                        spec,           // Tier-3 builder (Custom entry)
                        v[14] as u32,   // seeding strategy
                        v[15] as usize, // line seeds
                        v[16] as usize, // max RK4 steps per line
                        v[17],          // step length ds
                        v[18] > 0.5,    // bidirectional tracing
                        v[4],           // box half-extent (shared with arrows)
                        v[5],           // domain scale k
                        v[19] as u32,   // line colour (|F| / sweep)
                        v[22],          // line thickness
                        v[20],          // flow-pulse amount
                        v[21],          // flow-pulse speed
                        v[10],          // evolve (same rotating domain)
                        v[11],          // z lift (same lifted field)
                        self.gen_phase as f32,
                        palette,
                        self.color_phase as f32,
                        &mut self.geom.gen_strands,
                    );
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
                }
            }
            GeneratorMode::FieldEngine => {
                // #381 Tier 1: evaluate a closed-form field program over (x,y,z,t)
                // and render it through the existing viz machinery. Vector → RK4
                // field-lines (the shared volumetric duo tracer, one field in both
                // channels) + the Particle Aura (fill_analytic in the aura pass);
                // Scalar → a density/height glyph lattice; Complex → |ψ|² density
                // tinted by phase arg ψ. Streamlines topology, so every surface mode
                // reads it.
                use organon_core::params::FieldKind;
                // #381 Tier 3: when a PDE preset is selected, march a live grid sim
                // off the PLL beat clock and render its state instead of the static
                // analytic field. `Off` (default) falls through to Tier 1/2 unchanged.
                let sim_ps = math::PdePreset::from_u32(s.fieldsim[0] as u32);
                if sim_ps == math::PdePreset::Playback {
                    // #407 Tier A: replay a pre-baked `FieldClip` (from PolymathicAI's
                    // *The Well*) through the SAME glyph lattice as the live sim, stepped
                    // off the PLL beat clock. The clip is loaded from the sidecar path,
                    // edge-detected on `fieldclip_gen` (the `field_gen` pattern).
                    if self.field_clip_gen != s.fieldclip_gen {
                        self.field_clip_gen = s.fieldclip_gen;
                        self.field_clip_phase = 0.0;
                        self.field_clip_beat_prev = self.beat_pos;
                        let path = std::fs::read_to_string(ipc::field_clip_sidecar_path())
                            .map(|p| p.trim().to_string())
                            .unwrap_or_default();
                        self.field_clip = if path.is_empty() {
                            None
                        } else {
                            match std::fs::read(&path) {
                                Ok(bytes) => match math::FieldClip::from_bytes(&bytes) {
                                    Some(clip) => Some(clip),
                                    None => {
                                        eprintln!("field clip: malformed/rejected: {path:?}");
                                        None
                                    }
                                },
                                Err(e) => {
                                    eprintln!("field clip: read failed: {e} — {path:?}");
                                    None
                                }
                            }
                        };
                    }
                    // Advance the playback phase by the per-frame beat delta × the
                    // `time_scale` slot (fieldsim[2]) — the same clamp + host-stopped
                    // freeze the sim branch uses (so re-entry can't lurch the clip).
                    let time_scale = s.fieldsim[2].max(0.0);
                    let host_stopped = s.transport[3] != 0.0 && s.transport[0] == 0.0;
                    const MAX_CLIP_BEAT_STEP: f64 = 0.25;
                    let dbeat = (self.beat_pos - self.field_clip_beat_prev)
                        .clamp(0.0, MAX_CLIP_BEAT_STEP);
                    self.field_clip_beat_prev = self.beat_pos;
                    if !host_stopped {
                        self.field_clip_phase += dbeat * time_scale as f64;
                    }
                    if let Some(clip) = self.field_clip.as_ref() {
                        let n = clip.nframes().max(1);
                        // `as usize` saturates (never wraps) for a huge phase; % n is safe.
                        let frame = (self.field_clip_phase.floor().max(0.0) as usize) % n;
                        let extent = s.field[3].max(0.1);
                        let gain = s.field[7].max(0.0);
                        let thickness = s.field[8].max(1.0e-3);
                        math::field_clip_strands(
                            clip, frame, extent, thickness, gain, 0.0, palette,
                            self.color_phase as f32, &mut self.geom.gen_strands,
                        );
                    } else {
                        self.geom.gen_strands.restart();
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else if sim_ps == math::PdePreset::NeuralCa {
                    // #407 Tier B: a learned Neural Cellular Automaton rolls out live
                    // on a CPU grid (trained offline on The Well; built-in default when
                    // no model is loaded), rendered through the same glyph lattice as
                    // the FieldSim path. GPU dispatch (`nca.wgsl`) is deferred — the CPU
                    // rollout is authoritative (like FieldSim's deferred GPU route).
                    let fs = &s.fieldsim;
                    let time_scale = fs[2].max(0.0);
                    let res = match fs[7] as usize {
                        0 => 64,
                        r => r.clamp(16, 128),
                    };
                    // (Re)load weights + reseed when the model counter or resolution
                    // changes. Load from the sidecar, else the built-in default so it
                    // ALWAYS renders (empty / missing / malformed → default).
                    let key = (s.nca_gen, res);
                    if self.neural_ca.is_none() || self.neural_ca_key != key {
                        let weights = std::fs::read_to_string(ipc::nca_sidecar_path())
                            .ok()
                            .and_then(|txt| {
                                let p = txt.trim();
                                if p.is_empty() { None } else { std::fs::read_to_string(p).ok() }
                            })
                            .and_then(|json| math::NcaWeights::from_json(&json))
                            .unwrap_or_else(math::NcaWeights::builtin_default);
                        self.neural_ca = Some(math::NeuralCA::new(weights, res, 0x2749_1a3b));
                        self.neural_ca_key = key;
                        self.neural_ca_beat_prev = self.beat_pos;
                    }
                    // Advance by the per-frame beat delta (tempo-synced), same clamp +
                    // host-stopped freeze the FieldSim branch uses.
                    let host_stopped = s.transport[3] != 0.0 && s.transport[0] == 0.0;
                    const MAX_SIM_BEAT_STEP: f64 = 0.25;
                    let dbeat = (self.beat_pos - self.neural_ca_beat_prev)
                        .clamp(0.0, MAX_SIM_BEAT_STEP);
                    self.neural_ca_beat_prev = self.beat_pos;
                    if let Some(ca) = self.neural_ca.as_mut() {
                        let sim_dt = if host_stopped { 0.0 } else { dbeat as f32 * time_scale };
                        ca.advance(sim_dt);
                        let extent = s.field[3].max(0.1);
                        let gain = s.field[7].max(0.0);
                        let thickness = s.field[8].max(1.0e-3);
                        math::neural_ca_strands(
                            ca, extent, thickness, gain, 0.0, palette,
                            self.color_phase as f32, &mut self.geom.gen_strands,
                        );
                    } else {
                        self.geom.gen_strands.restart();
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else if sim_ps != math::PdePreset::Off {
                    let fs = &s.fieldsim;
                    let d = fs[1];
                    let time_scale = fs[2].max(0.0);
                    let feed = fs[3];
                    let kill = fs[4];
                    let potential = fs[5];
                    let forcing = fs[6];
                    // `0` (unset / old preset) means the default 64; otherwise clamp to [16,128].
                    let res = match fs[7] as usize {
                        0 => 64,
                        r => r.clamp(16, 128),
                    };
                    let key = (sim_ps.to_u32(), res);
                    // (Re)seed the sim when the preset or resolution changes.
                    if self.field_sim.is_none() || self.field_sim_key != key {
                        self.field_sim =
                            Some(math::FieldSim::new(sim_ps, res, d, feed, kill, potential));
                        self.field_sim_key = key;
                        self.field_sim_beat_prev = self.beat_pos;
                    }
                    // Advance by the per-frame beat delta (tempo-synced). Nice-to-have:
                    // freeze only when the host supplies tempo AND transport is stopped
                    // (standalone / no host tempo keeps running off the free-run clock).
                    let host_stopped = s.transport[3] != 0.0 && s.transport[0] == 0.0;
                    // Clamp the delta: `field_sim_beat_prev` only updates while this arm
                    // runs, so after the preset is Off (or another generator is active)
                    // `beat_pos` keeps advancing and re-entry would otherwise apply one
                    // huge gap at once (the grid lurching forward instead of resuming from
                    // frozen). 0.25 beat is far above any real per-frame delta yet bounds
                    // the gap — the sim resumes essentially where it froze.
                    const MAX_SIM_BEAT_STEP: f64 = 0.25;
                    let dbeat = (self.beat_pos - self.field_sim_beat_prev)
                        .clamp(0.0, MAX_SIM_BEAT_STEP);
                    self.field_sim_beat_prev = self.beat_pos;
                    if let Some(sim) = self.field_sim.as_mut() {
                        // Live coefficients mutate in place (no reseed needed).
                        sim.d = d.max(0.0);
                        sim.feed = feed;
                        sim.kill = kill;
                        sim.potential = potential.max(0.0);
                        let sim_dt = if host_stopped { 0.0 } else { dbeat as f32 * time_scale };
                        sim.advance(sim_dt, forcing);
                        let extent = s.field[3].max(0.1);
                        let gain = s.field[7].max(0.0);
                        let thickness = s.field[8].max(1.0e-3);
                        math::field_sim_strands(
                            sim, extent, thickness, gain, 0.0, palette,
                            self.color_phase as f32, &mut self.geom.gen_strands,
                        );
                    } else {
                        self.geom.gen_strands.restart();
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else {
                self.field_prog.ensure_field_program(&s);
                let f = &s.field;
                let scale = f[2].max(1.0e-3);
                let extent = f[3].max(0.1);
                let a = f[4];
                let b = f[5];
                // Clamp to a bounded range: `f32 as usize` saturates, so a corrupted /
                // out-of-range IPC value (the param is 1..64) would otherwise become a
                // huge seed count and blow up `volume_seeds` allocation + tracing.
                let seeds = (f[6] as usize).clamp(1, 4096);
                let gain = f[7].max(0.0);
                let thickness = f[8].max(1.0e-3);
                let color_phase = self.color_phase as f32;
                if let Some(base) = self.field_prog.field_program.clone() {
                    let prog = base
                        .with_bindings(self.gen_phase as f32, a, b)
                        .with_scale(scale);
                    let kind = match FieldKind::from_u32(f[0] as u32) {
                        FieldKind::Auto => prog.kind(),
                        FieldKind::Scalar => math::FieldValKind::Scalar,
                        FieldKind::Vector => math::FieldValKind::Vector,
                        FieldKind::Complex => math::FieldValKind::Complex,
                    };
                    match kind {
                        math::FieldValKind::Vector => {
                            let field = math::AnalyticField::Field { program: prog };
                            let colour = if palette == 0 {
                                Vec4::new(0.20, 0.85, 0.95, 1.0)
                            } else {
                                math::palette_tint(palette, 0.5 + color_phase)
                            };
                            let max_steps = 160usize;
                            let ds = (2.0 * extent / max_steps as f32).max(1.0e-3);
                            // Single field → trace each seed once (the duo tracer would
                            // draw every streamline twice).
                            math::field_lines_volumetric_strands(
                                &field, colour,
                                thickness, seeds, max_steps, ds,
                                Vec3::ZERO, extent, self.gen_phase as f32,
                                &mut self.geom.gen_strands,
                            );
                        }
                        math::FieldValKind::Scalar | math::FieldValKind::Complex => {
                            let complex = kind == math::FieldValKind::Complex;
                            let grid = seeds.clamp(1, 40);
                            // Pass gain as-is (already floored at 0) so the slider can fully
                            // suppress the lattice glyphs at 0; `field_lattice_strands` uses it
                            // as a length multiplier (no divide-by-zero).
                            math::field_lattice_strands(
                                &prog, complex, grid, extent, thickness,
                                gain, 0.0, palette, color_phase,
                                &mut self.geom.gen_strands,
                            );
                        }
                    }
                } else {
                    self.geom.gen_strands.restart();
                }
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
                } // end else: static Tier 1/2 field path
            }
            GeneratorMode::AxonWaveguide => {
                // #218 Tier 1: a bundle of myelinated-axon fibres as swept tubes,
                // with Ranvier-node constrictions + a travelling pulse. Streamlines
                // topology → view in Swept Tubes + Glass/Refractive. Tier 2: a guided
                // LP mode lights the bundle cross-section (a[12]/a[13]).
                let a = &s.axon;
                let phase = self.gen_phase as f32;
                math::axon_strands(
                    a[0] as usize, // fibre count
                    a[1],          // length
                    a[2],          // bundle radius
                    a[3] as usize, // samples / fibre
                    a[4],          // thickness
                    a[5],          // node spacing
                    a[6],          // node dip
                    a[7],          // pulse speed
                    a[8],          // pulse width
                    a[9],          // stagger
                    a[10],         // splay
                    a[11] as u32,  // seed
                    a[12] as u32,  // guided mode (Tier 2)
                    a[13],         // mode amount (Tier 2)
                    a[14],         // bend-degradation (Tier 3)
                    a[15],         // tract curve (C-arc)
                    a[16],         // tortuosity
                    a[17],         // DTI colour blend
                    a[18],         // dispersion (Tier 4)
                    a[19],         // polarization (Tier 4)
                    phase,
                    palette,
                    self.color_phase as f32,
                    &mut self.geom.gen_strands,
                );
                let fa = flow_aligned || strand_membrane_fallback;
                self.geom.emit_strands(fa, caps, weld)
            }
            GeneratorMode::None => {
                // #187 scenery pivot: the primary generator switched off — the
                // Scenery layer (below) and the world layers carry the scene.
                self.geom.instances.clear();
                self.geom.tints.clear();
                math::Bounds::new()
            }
            GeneratorMode::NeuralField => {
                // Dual-path (like Minimal surfaces): the Strand form (#200 Tier 1b)
                // samples the MLP on a grid and DISPLACES nodes — a real node field
                // every Surface mode + Material skins; the Raymarch form (Tier 1)
                // is the fullscreen isosurface with no nodes.
                let n3 = &s.neural2; // network identity/walk live in s.neural
                let strands_mode = s.neural3[0] != 0.0;
                if strands_mode {
                    let nf = &s.neural3;
                    math::neural_strands(
                        s.neural[1].max(0.0) as u32, // seed A
                        s.neural[2].max(0.0) as u32, // seed B
                        neural_walk_resolved,        // beat-driven latent walk (below)
                        s.neural[4].max(1e-3),       // omega (feature scale)
                        n3[1].max(1e-3),             // coord (detail) — shared with raymarch
                        (nf[1] as usize).clamp(2, 400), // strands (columns)
                        (nf[2] as usize).clamp(2, 400), // nodes (rows)
                        nf[3].max(1e-3) * breath_scale.x, // extent (breathes)
                        nf[4].max(0.0),              // displacement
                        (self.gen_phase * 0.5) as f32, // time (4th input)
                        n3[5].clamp(0.0, 1.0),       // colour intensity
                        &mut self.geom.gen_strands,
                    );
                    // Grid → membrane skins the displaced sheet (all columns lofted).
                    if membrane_mode && !membrane_arms && self.geom.gen_strands.len() >= 2 {
                        let mem = math::strands_to_mem(&self.geom.gen_strands);
                        let gx = mem.len();
                        math::loft_membrane(
                            &mem, gx, 1, 1, false, 0, membrane_close, palette, self.color_phase as f32,
                            &mut self.mem_pos, &mut self.mem_norm, &mut self.mem_col,
                            &mut self.mem_idx,
                        );
                        draw_membrane_mesh = true;
                    }
                    let fa = flow_aligned || strand_membrane_fallback;
                    self.geom.emit_strands(fa, caps, weld)
                } else {
                    // Raymarch form — no nodes. Clear the instanced buffers; its own
                    // raymarch path draws below. Bounds frame the field.
                    self.geom.instances.clear();
                    self.geom.tints.clear();
                    draw_membrane_mesh = false;
                    let scale = n3[0].max(1e-3) * breath_scale.x;
                    let r = Vec3::splat(scale * 1.3);
                    math::Bounds { min: -r, max: r }
                }
            }
            GeneratorMode::NeuralNetwork => {
                // #226: a graph of neuron soma (single-frame node blobs) + routed
                // fibre-tract edges (bowed swept tubes carrying a travelling pulse).
                // Streamlines topology → view in Swept Tubes + Glass for the
                // glowing-connectome look. Pure graph math (`neural_graph`) drives it.
                let nw = &s.neural_net;
                let nw2 = &s.neural_net2;
                let ne = &s.neural_edge; // Tier 1.5: axon-bundle edges + dendritic somas
                let nm = &s.neural_mlp; // Tier 4: MLP look dials
                let na = &s.neural_attn; // Tier 5: attention look dials
                let phase = self.gen_phase as f32;
                let topo = nw[0] as u32;
                let nodes = nw[1] as usize;
                let conn = nw[2] as usize;
                let rewire = nw[3];
                let layers = nw[4] as usize;
                let seed = nw[5] as u32;
                let extent = nw[6] * breath_scale.x; // breathes
                let fire_mode = nw2[0] as u32;
                // Tier 4: signed-weight edge colouring only for the MLP topology.
                let sign_colour = if topo == 6 { nm[0] } else { 0.0 };
                // Resolve the graph: topology 6 = the ingested MLP (#226 Tier 4, its
                // live forward pass builds the graph — input optionally beat-driven);
                // 5 = the ingested connectome (#226 Tier 3); else synthesized from the
                // bank. Owned per frame (cheap at these scales) so the sim + geometry
                // share one graph.
                let g = if topo == 7 {
                    // #226 Tier 5 — transformer attention: a loaded tensor (or the
                    // stylized causal synthesis) as a triangular attention graph.
                    // `reveal` grows over beats (token-by-token generation) and `sweep`
                    // auto-cycles the visualized head. Head/layer are clamped to the
                    // loaded tensor's shape; the synthetic pattern is unbounded.
                    let loaded = self.neural_attn_data.as_ref();
                    let n_tokens = match loaded {
                        Some(a) => a.n_tokens,
                        None => (na[3] as usize).max(2),
                    };
                    // Synthetic pattern has no fixed shape → give the sweep a plausible
                    // head/layer count to cycle through (stylistic variety only).
                    let n_layers = loaded.map(|a| a.n_layers.max(1)).unwrap_or(6);
                    let n_heads = loaded.map(|a| a.n_heads.max(1)).unwrap_or(8);
                    let sweep = (self.beat_pos * na[5] as f64).floor() as i64;
                    let layer = (na[0] as usize).min(n_layers.saturating_sub(1));
                    let head = ((na[1] as i64 + sweep).rem_euclid(n_heads as i64)) as usize;
                    // Reveal front: 0 rate → all tokens; else grow 1..=n and repeat.
                    let reveal = if na[4] > 0.0 {
                        let steps = (self.beat_pos * na[4] as f64).rem_euclid(n_tokens as f64);
                        (1 + steps as usize).min(n_tokens)
                    } else {
                        n_tokens
                    };
                    let ring = na[6] > 0.5;
                    math::attention_to_graph(
                        loaded, layer, head, n_tokens, reveal, extent, na[2], ring,
                        seed ^ self.neural_load_gen,
                    )
                } else if topo == 6 {
                    match &self.neural_mlp {
                        Some(m) => {
                            let drive = nm[3];
                            let n_in = m.layers.first().copied().unwrap_or(0);
                            let input: Vec<f32> = (0..n_in)
                                .map(|i| {
                                    let base = m.input.get(i).copied().unwrap_or(0.0);
                                    if drive > 0.0 {
                                        base + drive
                                            * ((self.beat_pos as f32) * 0.5 + i as f32 * 0.6).sin()
                                    } else {
                                        base
                                    }
                                })
                                .collect();
                            math::mlp_to_graph(m, &input, extent, nm[2], nm[1])
                        }
                        None => math::NeuralGraph::default(),
                    }
                } else if topo == 5 {
                    // #367 Tier 2 / #520 — the live glow. On the SPECIMEN view we poll
                    // the activation ring every frame and overwrite `node_scalar` from
                    // the latest frame, so the #226 node-glow fires per token. There is
                    // no "Live" mode to switch on: frames arriving ARE live, and their
                    // absence leaves the geometry static. Base graph: the loaded
                    // connectome if present, else a bare-dims architecture skeleton built
                    // from the frame (so the synthetic writer demos with NO .gguf loaded).
                    // The GALAXY view (`mind[2] == 1`) falls through to the `else`: its
                    // points are vocabulary tokens, and the ring carries per-LAYER
                    // activations, so there is no honest mapping between them yet — that
                    // is #507 Tier 2 (the residual trajectory on the cb_eval tap). Until
                    // then the galaxy is a true static view and generating never
                    // disturbs it.
                    if math::mind_view_mode(s.mind[2]) == 0 {
                        if !self.mind_ring.is_open() {
                            self.mind_ring = mind_ring::MindRingReader::open();
                        }
                        let frame = self.mind_ring.latest();
                        // Base graph MUST match the frame's dims 1:1 or the arch-order
                        // stream would light the wrong nodes. Reuse the loaded connectome
                        // ONLY when its node count equals the frame's architecture node
                        // count (a .gguf whose dims match the writer); otherwise (JSON
                        // connectome, or a specimen with different dims) build a bare-dims
                        // skeleton from the frame so `stream_frame_into_scalars` maps 1:1.
                        let mut g = match &frame {
                            Some(f) => {
                                let want = math::arch_node_count(f.n_layers, f.n_heads);
                                match &self.neural_loaded {
                                    Some(loaded) if loaded.nodes.len() == want => loaded.clone(),
                                    _ => math::architecture_skeleton_graph(
                                        f.n_layers, f.n_heads, extent,
                                    ),
                                }
                            }
                            None => self.neural_loaded.clone().unwrap_or_default(),
                        };
                        if let Some(f) = frame {
                            math::stream_frame_into_scalars(
                                &mut g.node_scalar,
                                f.n_layers,
                                f.n_heads,
                                mind_ring::MR_MAX_HEADS,
                                &f.layer_norm,
                                &f.mlp_act,
                                &f.head_summ,
                            );
                        }
                        g
                    } else {
                        self.neural_loaded.clone().unwrap_or_default()
                    }
                } else if topo == 8 {
                    // #275 — the brain model: two folded cerebral hemispheres split by
                    // a fissure + a cerebellum, wired short-range local cortex. The
                    // k-NN wiring is O(n²), so build once and CACHE (keyed on the
                    // dials), not per frame. Built at the unbreathed extent so the
                    // cache key is stable.
                    let br = &s.brain;
                    let bx = nw[6].max(1.0); // fixed extent (no breath) for a stable cache
                    let key = brain_cache_key(nodes, bx, br);
                    if self.brain_cache.as_ref().map(|(k, _, _)| *k) != Some(key) {
                        let g = math::brain_graph(
                            nodes, bx, br[0], br[1], br[2], br[3] as usize, br[4],
                            br[5], br[6], br[7], // T2: assoc tracts / corpus callosum / subcortical
                        );
                        let regions = math::brain_regions(&g.nodes, bx); // T3 parcellation
                        self.brain_cache = Some((key, g, regions));
                    }
                    // Clone the graph, then (T3) highlight the selected target region so
                    // its location on the cortex reads — the address the TMS/entrainment
                    // tools will aim at. `br[8]` = highlight amount, `br[9]` = region id.
                    let mut bg = self.brain_cache.as_ref().map(|(_, g, _)| g.clone()).unwrap_or_default();
                    if br[8] > 0.0 {
                        if let Some((_, _, regions)) = &self.brain_cache {
                            if !regions.is_empty() {
                                let idx = (br[9] as usize).min(regions.len() - 1);
                                for &m in &regions[idx].members {
                                    if let Some(v) = bg.node_scalar.get_mut(m) {
                                        *v = (*v + br[8]).min(2.0);
                                    }
                                }
                            }
                        }
                    }
                    bg
                } else {
                    math::neural_graph(topo, nodes, conn, rewire, layers, extent, seed)
                };
                if g.nodes.is_empty() {
                    // Connectome selected but nothing loaded yet → draw nothing.
                    self.geom.instances.clear();
                    self.geom.tints.clear();
                    self.neural_batches = None;
                    math::Bounds::new()
                } else {
                    // Compute the Tier-2 activity ONCE (None = the free-running Tier-1
                    // pulse) so both the swept-tube lowering (`neural_net_lay`) and the
                    // Neural-Tissue lowering (`neural_tissue_lay`) can ride it. #226 Tier
                    // 2 signal propagation: step the activation-cascade sim on the beat
                    // clock, then geometry is driven by the live activity (firing nodes
                    // flare, only edges carrying a real pulse light up). A loaded
                    // connectome flows through unchanged.
                    // #275 Tier 4 — focal stimulation: a coil-like drive at the target
                    // region turns the cascade sim ON even with no firing mode set (the
                    // stimulus IS the drive) and pulses the target region's neurons on the
                    // stim rate; the existing cascade + the corpus callosum then carry the
                    // effect across to the contralateral hemisphere.
                    let stim_on = topo == 8 && s.brain[10] > 0.0;
                    // #367 Tier 2 — the Live-streaming path lights nodes directly from the
                    // streamed `node_scalar` (the glow's `activity = None` branch). Keep the
                    // cascade sim OFF so the streamed scalars win, not a free-running pulse.
                    // #520 — "live" is a fact about the ring, not a mode: the sim is
                    // suppressed while real frames are driving the glow on the specimen.
                    let live_stream = topo == 5
                        && math::mind_view_mode(s.mind[2]) == 0
                        && self.mind_ring.latest().is_some();
                    // 🚨 #147 T3 — the same suppression, for the same reason, on the
                    // Delta lens. Its `node_scalar` is a **measured** quantity (how far
                    // each site moved during a fine-tune); the cascade sim would replace
                    // it with a free-running procedural pulse and the picture would then
                    // be neither — a proxy animation wearing a measurement's shape, which
                    // is precisely what this tier exists to make impossible. Unlike
                    // `live_stream` this needs no "are frames arriving" clause: the
                    // measurement is in the graph the moment the view is built.
                    let delta_lens = topo == 5 && math::mind_view_mode(s.mind[2]) == 2;
                    let sim_on = (fire_mode != 0 || stim_on) && !live_stream && !delta_lens;
                    let motes = if !sim_on {
                        0.0
                    } else {
                        let key = (topo, g.nodes.len(), g.edges.len(), layers,
                                   seed ^ self.neural_load_gen);
                        if self.neural_key != key {
                            self.neural_sim.rebuild(&g, seed);
                            self.neural_key = key;
                        }
                        let cfg = math::NeuralSimConfig {
                            mode: fire_mode,
                            threshold: nw2[1],
                            speed: nw2[2],
                            refractory: nw2[3],
                            decay: nw2[4],
                            deposit: nw2[5],
                            stim_rate: nw2[6],
                        };
                        if stim_on {
                            let rate = s.brain[11].max(0.0);
                            let tick = (self.beat_pos * rate as f64).floor() as i64;
                            if rate > 0.0 && tick != self.brain_stim_tick {
                                self.brain_stim_tick = tick;
                                // Drive each target-region neuron past threshold → it
                                // fires and propagates (br[10] = coil strength).
                                let amount = s.brain[10] + nw2[1];
                                if let Some((_, _, regions)) = &self.brain_cache {
                                    if !regions.is_empty() {
                                        let idx = (s.brain[9] as usize).min(regions.len() - 1);
                                        for &m in &regions[idx].members {
                                            self.neural_sim.stimulate(m, amount);
                                        }
                                    }
                                }
                            }
                        }
                        if s.animate != 0 {
                            self.neural_sim.step(dt_beats as f32, &cfg);
                        }
                        nw2[7]
                    };
                    let act = if sim_on { Some(self.neural_sim.activity()) } else { None };
                    if neural_tissue {
                        // #260 Tier 1 — closed anatomical primitives: one soma per node
                        // (degree-scaled icosphere), a capped capsule per edge (never an
                        // open pipe), a synaptic bouton bulb at each terminal. Lowered
                        // into three contiguous instance sub-batches drawn per-mesh.
                        let ns = &s.neural_surface;
                        let ns2 = &s.neural_surface2;
                        // #260 Tier 2 morphology dials ride ns[5..10] (dendrite
                        // density/length/taper, neuron type, spines); inert at density 0.
                        // #260 Tier 3 myelin dials ride ns[10..13] (myelin amount /
                        // Ranvier spacing / sheath scale); inert at amount 0.
                        // #260 Tier 4 (final) synapse dials ride ns[13..16] (cleft / glow
                        // / vesicles) + tissue context ns2[0..2] (glia / capillary); all
                        // inert at 0.
                        let (counts, b) = math::neural_tissue_lay(
                            &g, act.as_ref(), seed, nw[7], nw[8], nw[9],
                            ns[0], ns[1], ns[2],
                            ns[5], ns[6], ns[7], ns[8] as u32, ns[9],
                            ns[10], ns[11], ns[12],
                            ns[13], ns[14], ns[15],
                            ns2[0], ns2[1],
                            phase, palette, self.color_phase as f32,
                            // Signal swell: the Brain topology holds still (brain[12], default
                            // 0 = glow only) so stimulation lights up without the anatomy
                            // throbbing; every other topology keeps the 0.5 "living tissue" swell.
                            if topo == 8 { s.brain[12] } else { 0.5 },
                            &mut self.geom.instances, &mut self.geom.tints,
                        );
                        self.neural_batches = Some(render::NeuralBatches {
                            soma_count: counts.soma,
                            capsule_count: counts.capsule,
                            bouton_count: counts.bouton,
                        });
                        b
                    } else {
                        // Tier 1 / 1.5 — swept-tube fibre tracts; axon-bundle edges +
                        // dendritic somas ride the T1.5 slots.
                        math::neural_net_lay(
                            &g, act.as_ref(), seed, nw[7], nw[8], nw[9], nw[10], nw[11] as usize,
                            nw[12], nw[13], ne[0] as usize, ne[1], ne[2], ne[3], ne[4],
                            ne[5] as usize, motes, sign_colour, phase, palette,
                            self.color_phase as f32,
                            if topo == 8 { s.brain[12] } else { 0.5 },
                            &mut self.geom.gen_strands,
                        );
                        let fa = flow_aligned || strand_membrane_fallback;
                        self.geom.emit_strands(fa, caps, weld)
                    }
                }
            }
            GeneratorMode::Lens => {
                // #258 Tier 3: an analytic lens SDF — no nodes. Clear the instanced
                // buffers so nothing else draws; the lens is sphere-traced by its own
                // raymarch path below. Report bounds so the camera frames it (centre 0,
                // radius = world scale).
                self.geom.instances.clear();
                self.geom.tints.clear();
                draw_membrane_mesh = false;
                let scale = s.lens[4].max(1e-3) * breath_scale.x;
                let r = Vec3::splat(scale * 1.3);
                math::Bounds { min: -r, max: r }
            }
            GeneratorMode::Demo => {
                // #288 — a hand-authored reference scene. Emits explicit instanced
                // geometry (per-primitive mesh + material sub-batches) straight into
                // the instance/tint buffers; inherits shadows / TLAS / SSR / the path
                // tracer for free. `demo` = [scene, scale, objects, static_cam, light,
                // roughness, count, spin].
                let d = &s.demo;
                let scale = d[1].max(0.05) * breath_scale.x;
                let objects = d[2] >= 0.5;
                let light = d[4];
                let roughness = d[5];
                let count = d[6] as usize;
                let spin = d[7];
                let out = math::demo_scene(
                    d[0] as u32, scale, objects, light, roughness, count, spin,
                    self.gen_phase as f32, &mut self.geom.instances, &mut self.geom.tints,
                );
                self.demo_batches = out.batches;
                self.demo_lights = out.lights;
                out.bounds
            }
        };
        let mut bounds = bounds;

        // organon#217 T1 — the glyph ring drives the grid. When a producer is
        // publishing, the tiles REPLACE whatever the generator just emitted into
        // `instances`/`tints` (and fill `emits`), the way Plexus below replaces the node
        // cloud: the generator still ran (its node set may feed the particle stir
        // field), but the raster draws the text. With no ring — the default, and every
        // frame today — this is a `None` and nothing after it can tell the branch
        // exists. Sub-batches a Demo/Neural arm filled describe geometry that is no
        // longer in the buffer, so they are cleared exactly as Plexus clears them.
        // organon#217 T5: the same decision tells the path tracer whether the grid is
        // drawing, at which payload generation, and whether it is held — captured here,
        // once, so the tracer's gate and its restart cannot disagree with the geometry.
        self.glyph_pt = GlyphPtState::default();
        // organon#217 T3: the look comes off the param chain (`Shared.glyph`), and the
        // tiles' bounds are kept for the held camera (`glyph_camera_rig`).
        let glyph_look = glyph_look_from(&s);
        if let Some(b) = self.glyph_grid_geometry(&glyph_look, glyph_lower_options(&s.glyph)) {
            bounds = b;
            self.glyph_bounds = b;
            self.glyph_pt = GlyphPtState {
                live: true,
                generation: self.glyph_grid.frame.generation,
                settled: self.glyph_grid.settled(),
            };
            self.demo_batches.clear();
            self.demo_lights.clear();
            self.neural_batches = None;
            self.geom.swept_mesh.clear();
            // organon#217 T10, read for the record: clearing `rt_instances` is what puts
            // the tiles in the TLAS. The RT geometry choice below (`rt_geo`) is
            // `rt_instances` when that is non-empty and `instances` otherwise, and the
            // tiles + backplane ARE `instances` now — so every `rt_*` pass and the path
            // tracer trace the grid (the backplane is real geometry for exactly this,
            // §5), under the same gates as any instanced frame: the generator's path
            // is `Instanced`, it is not hidden, no boids creature. What the RT passes
            // cannot yet see is the emit buffer — T8 / `render.rs`.
            self.geom.rt_instances.clear();
            self.geom.rt_tints.clear();
        }

        // --- Plexus surface (#8, generator-agnostic) -----------------------
        // Whatever node cloud the active generator just emitted into `self.geom.instances`
        // (each instance's translation is a node centre) is rebuilt as a proximity
        // web: struts between near neighbours + a marker per node. Raymarch
        // generators leave `instances` empty, so this is a no-op for them.
        // `boids_creature >= 0` fills `instances` with per-agent CREATURE transforms,
        // not a node cloud, so Plexus must skip it (else it'd wire creature centres
        // into a web + clear the creature buffer). Boids in its non-creature form still
        // gets a plexus.
        let plex = &mut self.plexus;
        if plexus && boids_creature < 0 && !self.geom.instances.is_empty() {
            plex.nodes.clear();
            plex.ntints.clear();
            // organon#217 W17: while a ring is live the cloud IS the lowered grid, and a
            // node takes the tile's EMISSION, not its faceplate tint — this loop used to
            // copy `tints` and drop `emits`, which is why `bottled` / `cathode` came up
            // grey (`plexus_node_colour`). The gate is the renderer's own parallel-buffer
            // convention (`emits.len() == instances.len()`, else no emission); `lower_grid`
            // is the only filler of all three, so a live ring always passes it, and a
            // generator frame (no ring, `emits` empty) keeps its tint byte for byte.
            let glyph_emits = self.glyph_pt.live && self.geom.emits.len() == self.geom.instances.len();
            for (i, (m, t)) in self.geom.instances.iter().zip(self.geom.tints.iter()).enumerate() {
                plex.nodes.push(m.w_axis.truncate());
                let e = if glyph_emits { self.geom.emits[i] } else { Vec4::ZERO };
                plex.ntints.push(plexus_node_colour(glyph_emits, *t, e));
            }
            // Plexus replaces the instance buffer, so any Demo/Neural sub-batches the
            // generator emitted no longer describe it — clear them, or the renderer
            // would draw stale sub-batches (old counts / meshes) over the rebuilt
            // geometry. No-op unless a Demo/Neural arm actually filled them.
            self.demo_batches.clear();
            self.demo_lights.clear();
            self.neural_batches = None;
            let opts = math::PlexusOpts {
                radius_mul: s.plexus[0],
                max_links: s.plexus[1].max(1.0) as usize,
                strut_mul: s.plexus[2],
                marker_mul: s.plexus[3],
            };
            if s.plexus2[0] != 0.0 {
                // Tier 2: build node (sphere) + edge (tube) impostor lists from the
                // graph; the raster cube path draws nothing (cleared below).
                let g = math::plexus_graph(&plex.nodes, &plex.ntints, &opts);
                let node_r = (s.plexus2[2] * g.spacing).max(1e-4);
                let edge_r = (s.plexus2[3] * g.spacing).max(1e-4);
                let mut b = math::Bounds::new();
                for &p in &g.points {
                    b.min = b.min.min(p);
                    b.max = b.max.max(p);
                }
                // Tier 3 signal propagation: a bright activation shell radiates from the
                // web centre on the beat clock, firing the impostors it crosses. Each
                // node's activation is a Gaussian of its (normalized) radius vs. the
                // wavefront. Precompute per-node activation so edges can average it.
                let signal_on = s.plexus3[0] != 0.0;
                let centre = if b.min.x.is_finite() { (b.min + b.max) * 0.5 } else { Vec3::ZERO };
                let max_r = g.points.iter().fold(1e-4f32, |m, &p| m.max((p - centre).length()));
                let wave = ((self.beat_pos * s.plexus3[1] as f64).rem_euclid(1.0)) as f32;
                let gain = s.plexus3[2];
                let width = s.plexus3[3];
                let act = |p: Vec3| -> f32 {
                    if !signal_on {
                        return 0.0;
                    }
                    math::plexus_signal((p - centre).length() / max_r, wave, width)
                };
                plex.activations.clear();
                for &p in &g.points {
                    plex.activations.push(act(p));
                }
                for (i, &p) in g.points.iter().enumerate() {
                    let t = g.tints[i];
                    let boost = 1.0 + gain * plex.activations[i];
                    // Node sphere = a degenerate (A≈B) capsule of radius `node_r`.
                    plex.node_caps.push(render::MembraneArmInstance {
                        a_r: [p.x, p.y, p.z, node_r],
                        b: [p.x, p.y, p.z, 0.0],
                        color: [t.x * boost, t.y * boost, t.z * boost, 1.0],
                    });
                }
                if s.plexus2[1] != 0.0 {
                    for &(a, c) in &g.edges {
                        let pa = g.points[a as usize];
                        let pc = g.points[c as usize];
                        let t = (g.tints[a as usize] + g.tints[c as usize]) * 0.5;
                        let ea = plex.activations[a as usize];
                        let eb = plex.activations[c as usize];
                        let boost = 1.0 + gain * 0.5 * (ea + eb);
                        plex.edge_caps.push(render::MembraneArmInstance {
                            a_r: [pa.x, pa.y, pa.z, edge_r],
                            b: [pc.x, pc.y, pc.z, 0.0],
                            color: [t.x * boost, t.y * boost, t.z * boost, 1.0],
                        });
                    }
                }
                self.geom.instances.clear();
                self.geom.tints.clear();
                // Impostor mode empties `instances`, but the node-driven systems (VXGI,
                // emissive many-lights, bounced GI) fall back to the welded node anchors
                // when `instances` is empty. Feed them the plexus node set so the web
                // still voxelizes / lights. Gated on `need_weld_nodes` (else those
                // systems are off and this is wasted work).
                if self.geom.need_weld_nodes {
                    self.geom.node_insts_weld.clear();
                    self.geom.node_tints_weld.clear();
                    for (i, &p) in g.points.iter().enumerate() {
                        self.geom.node_insts_weld.push(Mat4::from_translation(p));
                        self.geom.node_tints_weld.push(g.tints[i]);
                    }
                }
                bounds = b;
            } else {
                // Tier 1: rebuild the instance buffer as markers + struts, drawn as two
                // shape-morphed sub-batches (node cube→sphere, strut square→circle).
                // Rebuild the morph meshes only when a shape slider changes (global
                // param → one small mesh each, not per-instance).
                let shape = (s.plexus4[0], s.plexus4[1]);
                if shape != plex.shape_cache || plex.node_mesh.is_empty() {
                    plex.node_mesh = math::morph_cube_mesh(shape.0);
                    plex.edge_mesh = math::morph_strut_mesh(shape.1);
                    plex.shape_cache = shape;
                }
                let tier1 = math::draw_plexus(
                    &plex.nodes,
                    &plex.ntints,
                    opts,
                    &mut self.geom.instances,
                    &mut self.geom.tints,
                );
                bounds = tier1.bounds;
                plex.batches = Some(render::PlexusBatches {
                    markers: tier1.markers as u32,
                    struts: tier1.struts as u32,
                });
                // `instances` now holds markers AND strut midpoints. The node-driven
                // systems (VXGI, emissive many-lights, GI) would otherwise voxelize/
                // light those strut midpoints, not the node centres. Feed them the pure
                // node set via the welded-anchor path (preferred whenever populated).
                if self.geom.need_weld_nodes {
                    self.geom.node_insts_weld.clear();
                    self.geom.node_tints_weld.clear();
                    for (i, &p) in plex.nodes.iter().enumerate() {
                        self.geom.node_insts_weld.push(Mat4::from_translation(p));
                        self.geom.node_tints_weld.push(plex.ntints[i]);
                    }
                }
            }
        }

        // --- Plexus OVERLAY (outer shell around ANOTHER surface) -----------
        // Like the Particle Aura / Water, this reads whatever node cloud the active
        // generator emitted into `self.geom.instances` WITHOUT replacing it, so the base
        // surface keeps rendering. It keeps only the cloud's outer shell (`outer_shell`
        // — the rind, not the full volume), grows it outward into a cage, and wires it
        // as a plexus web using the SAME look params the standalone surface uses. Only
        // runs when the surface is NOT already Plexus (`!plexus` — else it'd double the
        // web) and a base node cloud exists (raymarch generators leave it empty). Boids
        // creatures aren't a node cloud, so skip them (same guard as the surface path).
        let plexus_overlay_on = s.plexus_overlay[0] != 0.0;
        // Source the base node cloud like the aura/ink do: `instances`, or the welded
        // per-segment anchors when Contiguous Swept Tubes cleared `instances` (filled by
        // `lower_strands` because `plexus_overlay_on` is now in `need_weld_nodes`) — so
        // the overlay isn't silently skipped in welded mode.
        let (ov_src_inst, ov_src_tint): (&[Mat4], &[Vec4]) = if !self.geom.instances.is_empty() {
            (&self.geom.instances, &self.geom.tints)
        } else {
            (&self.geom.node_insts_weld, &self.geom.node_tints_weld)
        };
        if plexus_overlay_on && !plexus && boids_creature < 0 && !ov_src_inst.is_empty() {
            let ov_scale = s.plexus_overlay[1];
            let ov_band = s.plexus_overlay[2]; // radial-band depth (fraction of the rim)
            let ov_bins = s.plexus_overlay[3].max(1.0) as usize;
            // Sub-sample the base node cloud to a FIXED count (bounded, stable index
            // order). An integer stride (`len / CAP`) makes the sample-set SIZE jump as
            // an animating generator's node count crosses a CAP multiple — n = 12000 →
            // stride 1 → 12000, n = 12001 → stride 2 → 6001 — halving the set in one
            // frame. That both changes `scores.len()` (force-resetting the shell-membership
            // EMA below) and reshuffles which nodes `shell_scores` sees, popping the cage.
            // Selecting exactly `min(n, CAP)` evenly-spread nodes keeps the sample size
            // continuous across the boundary (mirrors the NODE_CAP fix in `plexus_graph`).
            plex.ov_sample_nodes.clear();
            plex.ov_sample_tints.clear();
            const OV_SAMPLE_CAP: usize = 6000;
            let ov_n = ov_src_inst.len();
            let ov_target = ov_n.min(OV_SAMPLE_CAP);
            for j in 0..ov_target {
                let idx = j * ov_n / ov_target; // evenly spread over [0, n); always `target` nodes
                plex.ov_sample_nodes.push(ov_src_inst[idx].w_axis.truncate());
                plex.ov_sample_tints.push(ov_src_tint[idx]);
            }
            // Extract the shell with a TEMPORALLY-SMOOTHED membership: score each node's
            // shell-ness (r / rim), EMA it per stable sample index, then threshold the
            // smoothed value. A node hovering at the rim stops churning in/out each frame
            // (which was reconfiguring the graph), while a node that genuinely migrates to
            // the surface still joins once its smoothed score crosses. Reset (seed with the
            // raw scores) whenever the sample count changes so the index keying stays valid.
            let (centroid, scores) = math::shell_scores(&plex.ov_sample_nodes, ov_bins);
            const SHELL_EMA: f32 = 0.12; // ~0.15 s time constant at 60 fps
            if plex.ov_shellness.len() != scores.len() {
                plex.ov_shellness.clear();
                plex.ov_shellness.extend_from_slice(&scores);
            } else {
                for (sm, &raw) in plex.ov_shellness.iter_mut().zip(scores.iter()) {
                    *sm += (raw - *sm) * SHELL_EMA;
                }
            }
            plex.ov_nodes.clear();
            plex.ov_tints.clear();
            let keep = 1.0 - ov_band.clamp(1e-3, 1.0); // outer `band` fraction (smoothed)
            for i in 0..plex.ov_sample_nodes.len() {
                if plex.ov_shellness[i] >= keep {
                    let p = plex.ov_sample_nodes[i];
                    plex.ov_nodes.push(centroid + (p - centroid) * ov_scale);
                    plex.ov_tints.push(plex.ov_sample_tints[i]);
                }
            }
            if !plex.ov_nodes.is_empty() {
                let opts = math::PlexusOpts {
                    radius_mul: s.plexus[0],
                    max_links: s.plexus[1].max(1.0) as usize,
                    strut_mul: s.plexus[2],
                    marker_mul: s.plexus[3],
                };
                if s.plexus2[0] != 0.0 {
                    // Tier-2/3 impostor overlay: build the node/edge capsule caps from the
                    // shell graph (mirrors the standalone Tier-2 path, but leaves
                    // `instances`/bounds/weld anchors alone — the base surface owns them).
                    let g = math::plexus_graph(&plex.ov_nodes, &plex.ov_tints, &opts);
                    let node_r = (s.plexus2[2] * g.spacing).max(1e-4);
                    let edge_r = (s.plexus2[3] * g.spacing).max(1e-4);
                    let mut b = math::Bounds::new();
                    for &p in &g.points {
                        b.min = b.min.min(p);
                        b.max = b.max.max(p);
                    }
                    let signal_on = s.plexus3[0] != 0.0;
                    let centre = if b.min.x.is_finite() { (b.min + b.max) * 0.5 } else { Vec3::ZERO };
                    let max_r = g.points.iter().fold(1e-4f32, |m, &p| m.max((p - centre).length()));
                    let wave = ((self.beat_pos * s.plexus3[1] as f64).rem_euclid(1.0)) as f32;
                    let gain = s.plexus3[2];
                    let width = s.plexus3[3];
                    let act = |p: Vec3| -> f32 {
                        if !signal_on {
                            return 0.0;
                        }
                        math::plexus_signal((p - centre).length() / max_r, wave, width)
                    };
                    plex.activations.clear();
                    for &p in &g.points {
                        plex.activations.push(act(p));
                    }
                    for (i, &p) in g.points.iter().enumerate() {
                        let t = g.tints[i];
                        let boost = 1.0 + gain * plex.activations[i];
                        plex.node_caps.push(render::MembraneArmInstance {
                            a_r: [p.x, p.y, p.z, node_r],
                            b: [p.x, p.y, p.z, 0.0],
                            color: [t.x * boost, t.y * boost, t.z * boost, 1.0],
                        });
                    }
                    if s.plexus2[1] != 0.0 {
                        for &(a, c) in &g.edges {
                            let pa = g.points[a as usize];
                            let pc = g.points[c as usize];
                            let t = (g.tints[a as usize] + g.tints[c as usize]) * 0.5;
                            let ea = plex.activations[a as usize];
                            let eb = plex.activations[c as usize];
                            let boost = 1.0 + gain * 0.5 * (ea + eb);
                            plex.edge_caps.push(render::MembraneArmInstance {
                                a_r: [pa.x, pa.y, pa.z, edge_r],
                                b: [pc.x, pc.y, pc.z, 0.0],
                                color: [t.x * boost, t.y * boost, t.z * boost, 1.0],
                            });
                        }
                    }
                } else {
                    // Tier-1 overlay: markers+struts into the overlay's OWN instance
                    // buffers (the renderer layers them over the base surface). Reuses
                    // the shared shape-morph meshes (rebuilt only on a shape change).
                    let shape = (s.plexus4[0], s.plexus4[1]);
                    if shape != plex.shape_cache || plex.node_mesh.is_empty() {
                        plex.node_mesh = math::morph_cube_mesh(shape.0);
                        plex.edge_mesh = math::morph_strut_mesh(shape.1);
                        plex.shape_cache = shape;
                    }
                    let tier1 = math::draw_plexus(
                        &plex.ov_nodes,
                        &plex.ov_tints,
                        opts,
                        &mut plex.ov_insts,
                        &mut plex.ov_itints,
                    );
                    plex.overlay_batches = Some(render::PlexusBatches {
                        markers: tier1.markers as u32,
                        struts: tier1.struts as u32,
                    });
                }
            }
        }

        // --- Membrane Skin-Arms (generator-agnostic surface) ---------------
        // Skin each strand (arm) as its own closed capped finger with gaps between
        // arms — the volume-render hull — instead of one continuous shell. Works for
        // EVERY generator the membrane supports, off the shared strand set: the Mesh
        // build already happened via `weld` (each generator's swept-tube path), so
        // here we only (a) suppress the shell sheet so the fingers show, and (b) build
        // the Impostor capsules from the universal `gen_strands`. The Original cube-
        // field is pv-based and doesn't fill `gen_strands`, so produce its node strands.
        if membrane_mode && membrane_arms {
            self.mem_idx.clear(); // never draw the shell over the arms (kills a stale mesh)
            draw_membrane_mesh = false;
            if !membrane_arm_mesh {
                // Capsule radius: the Arm Radius dial (0 = auto). The Original cube-
                // field has no per-node thickness, so it seeds its strand radius from
                // the dial (default 0.5 of the unit grid step when auto).
                let arm_radius = s.membrane_fx[2].max(0.0);
                // Original is pv-based (never fills `gen_strands`), so regenerate its
                // node strands EVERY frame — NOT gated on `is_empty` — so the arms
                // animate as `angle` advances and any stale strands left by a
                // previously-selected strand generator are evicted (cube_field_strands
                // clears `gen_strands` first) rather than being skinned as Original's.
                if generator == GeneratorMode::Original {
                    let seed_r = if arm_radius > 0.0 { arm_radius } else { 0.5 };
                    math::cube_field_strands(
                        &pv, rot_func, trans_func, self.angle, rot_phase, continuous,
                        centered, seed_r, palette, self.color_phase as f32, &mut self.geom.gen_strands,
                    );
                }
                if !self.geom.gen_strands.is_empty() {
                    let b = self.geom.build_arm_caps(arm_radius, &mut self.arm_caps);
                    // Only take over the scene once we actually have capsules; if
                    // build_arm_caps produced none (e.g. every strand had < 2 nodes),
                    // leave the instanced-rod / swept fallback so the frame isn't blank.
                    if !self.arm_caps.is_empty() {
                        bounds = b;
                        self.geom.instances.clear();
                        self.geom.tints.clear();
                        self.geom.swept_mesh.clear();
                    }
                }
            }
        }

        // --- Scenery layer (#187 pivot) ------------------------------------
        // A second generator category running CONCURRENTLY with the primary
        // generator, rendered with its OWN material/surface via a second
        // uniform set in the renderer. Zone = the whole rails machinery (the
        // beat-parametrized window, per-cell + per-phrase morphing, the
        // quantized-transition latch, ribs, fade), moved here from the retired
        // GeneratorMode::Rails arm. Scenery renders via the instanced trio
        // (cubes / flow rods / swept tubes); the scenery membrane loft is a
        // follow-up, so the old mixed-topology handling collapses away.
        let scenery_on = self.rails_active;
        let scenery_surf = s.scenery[1] as u32;
        // Tubes for Swept Tubes (2) AND Skin (3): Skin's Streamlines archetypes
        // (Gates) can't loft and fall back to the instanced bridges, which the
        // `ScenerySurface` docs say degrade to swept tubes — so that fallback must
        // draw cylinders, not flow-aligned cube rods (#217 review).
        let scenery_tube = scenery_on && (scenery_surf == 2 || scenery_surf == 3);
        // Skin (#206 Tier 1): loft the corridor's Grid strands into a
        // continuous membrane drawn with the scenery material.
        let scenery_skin = scenery_on && scenery_surf == 3;
        // Composite vs pure ride (#187 camera fix): with a generator on screen
        // the orbit rig keeps flying the camera and the corridor is a
        // VIEW-LOCKED backdrop (see the scenery view-proj below); only the
        // generator-less ride hands the camera to the rails rig.
        self.rails_ride = scenery_on && generator == GeneratorMode::None;
        self.scenery_mem_pos.clear();
        self.scenery_mem_norm.clear();
        self.scenery_mem_col.clear();
        self.scenery_mem_idx.clear();
        self.water_mem_pos.clear();
        self.water_mem_norm.clear();
        self.water_mem_col.clear();
        self.water_mem_idx.clear();
        if !scenery_on {
            self.scenery_instances.clear();
            self.scenery_tints.clear();
        } else {
            let fresh = !self.rails_was_on;
            self.rails_was_on = true;
            // The combined latched block: rails timing/shape [0..24] ++ Terra
            // landform [24..40] (#206). Both quantize on the bar together.
            let mut live = [0.0f32; 40];
            live[..24].copy_from_slice(&s.rails);
            live[24..].copy_from_slice(&s.terra);
            rails_latch_step(
                &mut self.scenery_active_blk,
                &mut self.scenery_pending,
                &live,
                self.beat_pos,
                fresh,
            );
            self.rails_bore = math::RailsSpec::from_slots(&self.scenery_active_blk[..24]).bore;
            let is_terra = s.scenery[0] as u32 == 2;
            let sc_palette = s.scenery[8] as u32;
            // Skin lofts Grid strands; the instanced trio bridges successors for
            // rods/tubes (surface >= 1). A Streamlines archetype under Skin has
            // no loft → fall back to swept tubes so the corridor never vanishes.
            let sc_fa = scenery_surf >= 1;
            self.scenery_instances.clear();
            self.scenery_tints.clear();
            let cp = self.color_phase as f32;
            let beat_frac = self.beat_pos.fract() as f32;
            let gph = self.gen_phase as f32;
            let u_now = self.beat_pos;
            let mut sb = math::Bounds::new();

            // Generate + place one world's window (into `out`), then skin it
            // (Skin + Grid) or lower it to instances. `pending` = the two-worlds
            // transition, so vertex ranges append.
            let (active_blk, pending) = (self.scenery_active_blk, self.scenery_pending);
            let spans: [(&[f32; 40], (f64, f64)); 2] = match pending {
                None => [(&active_blk, math::RAILS_FULL_SPAN), (&active_blk, (0.0, 0.0))],
                Some((ref p, b)) => [(&active_blk, (f64::NEG_INFINITY, b)), (p, (b, f64::INFINITY))],
            };
            for (slot, (blk, span)) in spans.into_iter().enumerate() {
                if slot == 1 && pending.is_none() {
                    break; // single world
                }
                let out = if slot == 0 { &mut self.scenery_strands } else { &mut self.gen_strands_b };
                let (topo, dims) =
                    gen_scenery_world(blk, is_terra, u_now, span, beat_frac, gph, sc_palette, cp, out);
                for f in out.iter().flatten() {
                    sb.min = sb.min.min(f.position);
                    sb.max = sb.max.max(f.position);
                }
                // Disjoint field borrows: read the just-filled strand buffer,
                // write the scenery_mem_* / instance buffers.
                if slot == 0 {
                    let lofted = scenery_skin
                        && loft_scenery_append(
                            &self.scenery_strands, dims, topo, sc_palette, cp,
                            &mut self.scenery_mem_pos, &mut self.scenery_mem_norm,
                            &mut self.scenery_mem_col, &mut self.scenery_mem_idx,
                        );
                    if !lofted {
                        math::lower_strands_append(
                            &self.scenery_strands, sc_fa,
                            &mut self.scenery_instances, &mut self.scenery_tints, &mut sb,
                        );
                    }
                } else {
                    let lofted = scenery_skin
                        && loft_scenery_append(
                            &self.gen_strands_b, dims, topo, sc_palette, cp,
                            &mut self.scenery_mem_pos, &mut self.scenery_mem_norm,
                            &mut self.scenery_mem_col, &mut self.scenery_mem_idx,
                        );
                    if !lofted {
                        math::lower_strands_append(
                            &self.gen_strands_b, sc_fa,
                            &mut self.scenery_instances, &mut self.scenery_tints, &mut sb,
                        );
                    }
                }
                // Scenery water floor (#206 Tier 3): a rippled sheet at the
                // per-cell water level, spanning the valley — its own membrane
                // with its own (glass) material. Terra-only; `water_strands`
                // returns empty when the terra block's water is off. Ripple
                // scrolls with the (host-locked) beat clock so it moves in time.
                if is_terra {
                    let spec = math::RailsSpec::from_slots(&blk[..24]);
                    let terra = math::TerraSpec::from_slots(&blk[24..]);
                    let topo = math::water_strands(
                        &spec, &terra, s.water[5], s.water[6], u_now as f32, u_now, span,
                        &mut self.water_strands,
                    );
                    loft_scenery_append(
                        &self.water_strands, (spec.ring_n.max(3), 1), topo, 0, cp,
                        &mut self.water_mem_pos, &mut self.water_mem_norm,
                        &mut self.water_mem_col, &mut self.water_mem_idx,
                    );
                    // The loft colours by the RGB cube (palette 0), which the
                    // dedicated water shader would read as albedo → a rainbow
                    // sheet (#227 review). Refill with the water's own frame tint
                    // (the calm teal `water_col`), so the shader's depth/deep-colour
                    // work off a proper water albedo.
                    if let Some(f) = self.water_strands.iter().flatten().next() {
                        let tint = f.tint;
                        for c in self.water_mem_col.iter_mut() {
                            *c = tint;
                        }
                    }
                }
            }
            // Scenery joins the scene AABB only in the PURE RIDE (where rail
            // space is world space). In composite mode the corridor is
            // view-locked — folding its rail-space extent into the framing
            // bounds is what yanked cam_center down the tunnel (and tweened it
            // back on disable), so the orbit rig frames the generator alone.
            if self.rails_ride && sb.min.is_finite() {
                bounds.min = bounds.min.min(sb.min);
                bounds.max = bounds.max.max(sb.max);
            }
        }
        // Generator None + scenery off leaves an empty AABB — keep downstream
        // consumers (metaball extents, VXGI, shadow fit) NaN-free.
        if !bounds.min.is_finite() || !bounds.max.is_finite() {
            bounds = math::Bounds { min: Vec3::splat(-1.0), max: Vec3::splat(1.0) };
        }
        // Console Spike Tier 1: latched off while a substrate rig is installed. That rig
        // supplies its own centre, so the lerp would not move this frame's camera — but it
        // would keep integrating against the field's AABB and then resume mid-flight the
        // moment the rig is cleared. A plane centred on the origin makes the auto-follow a
        // fixed point *by coincidence*; this is the part that does not depend on that.
        if self.substrate_rig.is_none() {
            self.cam_center = self.cam_center.lerp(bounds.center(), 0.05);
        }

        // Metaball mode (surface_mode 3): turn the node set into field points. The
        // colour is the per-node tint when a palette/HSV sweep is active, else the
        // RGB colour cube by position in the field (so Native blobs stay colourful
        // and the existing palette/sweep colours carry straight through).
        // Mandelbulb generator (9): its own raymarch path; surface mode / metaball
        // / membrane don't apply (it builds no nodes), so gate them off.
        let mandelbulb = generator == GeneratorMode::Mandelbulb;
        // Creature Engine (#476): its own raymarch path; surface mode / metaball /
        // membrane don't apply (it builds no nodes), so gate them off like Mandelbulb.
        let creature_on = generator == GeneratorMode::Creature;
        // Minimal Surfaces (#127) is dual-path: the implicit TPMS families (0..2)
        // raymarch (no nodes); the parametric families (≥ 3, Phase 2) build a (u,v)
        // Grid and ride the normal instanced / membrane path, so only the implicit
        // case sets the raymarch flag + gates the surface modes off.
        let minimal = generator == GeneratorMode::MinimalSurface
            && !math::minimal_is_parametric(s.minimal_surface[0] as u32);
        let kifs_on = generator == GeneratorMode::Kaleidoscope;
        // Neural field (#200 Tier 1): the RAYMARCH form is its own path with no
        // nodes (gate surface modes off); the STRAND form (Tier 1b, neural3[0]≠0)
        // builds a node field, so it rides the normal instanced/membrane path.
        let neural_field_on = generator == GeneratorMode::NeuralField && s.neural3[0] == 0.0;
        // Lens (#258 Tier 3): its own raymarch path (an analytic lens SDF, no nodes),
        // so surface mode / metaball / membrane don't apply — gate them off.
        let lens_on = generator == GeneratorMode::Lens;
        // A Boids creature form overrides the surface mode entirely (one creature
        // per agent), so metaball/voxel are off when it's active.
        // `!draw_membrane_mesh` lets a generator that builds its own membrane mesh
        // (Tessellation's Filled/Extruded view) win over the surface mode — without
        // it, surface_mode 3/5 would select the metaball/voxel path, which reads the
        // now-cleared node set and draws nothing, hiding the tile mesh.
        let metaball = s.surface_mode == 3
            && !mandelbulb
            && !creature_on
            && !minimal
            && !kifs_on
            && !neural_field_on
            && !lens_on
            && boids_creature < 0
            && !draw_membrane_mesh;
        // Voxel mode (surface_mode 5): the Eulerian render path. Reuses the SAME
        // node set as metaball (splatted into a 3D grid instead of a metaball field).
        let voxel = s.surface_mode == 5
            && !mandelbulb
            && !creature_on
            && !minimal
            && !kifs_on
            && !neural_field_on
            && !lens_on
            && boids_creature < 0
            && !draw_membrane_mesh;
        // Volume mode (surface_mode 6, #152): reuses the metaball field bake but
        // raymarches it as a glowing participating medium (fs_volume). Same node set.
        let volume = s.surface_mode == 6
            && !fv_lines_active
            && !mandelbulb
            && !creature_on
            && !minimal
            && !kifs_on
            && !neural_field_on
            && !lens_on
            && boids_creature < 0
            && !draw_membrane_mesh;
        // Voxel GI (#152 Tier 3, #10): needs the node set voxelized regardless of the
        // surface mode, so build `meta_nodes` when it's on too.
        let vxgi_on = s.vxgi[0] != 0.0;
        // #167 Tier 3 (emissive cubes as lights) also needs the node set built.
        let manylight_on = s.manylight[0] != 0.0;
        if metaball || voxel || volume || vxgi_on || manylight_on {
            self.meta_nodes.clear();
            let palette_active = s.surface_fx[6] != 0.0;
            let span = (bounds.max - bounds.min).max(Vec3::splat(1e-3));
            // The per-node splat/influence radius differs by mode.
            let radius = if voxel {
                s.voxel[2]
            } else if volume {
                // Field Volume SmoothedNode / Auto-on-node-generator widen the kernel so the
                // node bake reads as a soft cloud, not the metaball skin. `field.wgsl` uses
                // each node's `pos.w` as the influence radius (NOT meta_params.radius), so the
                // widening MUST be applied here too, matching the `meta_params.radius` scale
                // below — else SmoothedNode stays as scraggly as Legacy. (Bugbot #5)
                use organon_core::params::FieldVolSource;
                let src = FieldVolSource::from_u32(s.fieldvol[0] as u32);
                let is_field = generator == GeneratorMode::MaxwellField
                    || generator == GeneratorMode::Acoustic;
                let smooth = if matches!(src, FieldVolSource::SmoothedNode)
                    || (matches!(src, FieldVolSource::Auto | FieldVolSource::FieldBaked)
                        && !is_field)
                {
                    s.fieldvol[1].max(0.0)
                } else {
                    1.0
                };
                s.volume[0] * smooth
            } else {
                s.metaball[0]
            };
            // Contiguous (welded) Swept Tubes clears `instances` (the raster draws the
            // welded mesh), so fall back to the per-segment welded node anchors — filled
            // by `emit_strands` when `need_weld_nodes` (which now includes VXGI /
            // many-lights) — so the field still voxelizes / lights in welded mode.
            let (src_inst, src_tint): (&[Mat4], &[Vec4]) =
                if !self.geom.node_insts_weld.is_empty() {
                    (&self.geom.node_insts_weld, &self.geom.node_tints_weld)
                } else {
                    (&self.geom.instances, &self.geom.tints)
                };
            if self.glyph_pt.live {
                // organon#217 T10 — glyphs as lights. The tiles replaced the generator's
                // instances above, so this node set would otherwise be every tile
                // (backplane included) coloured by the faceplate tint or by position —
                // and the "brightest N" point lights would be the top-right corner of
                // the grid. Lower the EMISSION instead: one candidate per run of lit
                // tiles, on the front face, coloured by the linear radiance the surface
                // itself emits (`glyph_light_candidates`). Off ReSTIR the set is
                // pre-trimmed to the preset's count by the same luminance the renderer
                // ranks by, so its own select is the identity; under ReSTIR it gets the
                // whole pool to rotate through. The per-node radius stays the surface
                // mode's (the lights read the uniform radius, not `pos.w`).
                let cell_h = glyph_look.cell_w
                    * if self.glyph_grid.cell_aspect > 0.0 { self.glyph_grid.cell_aspect } else { glyph_ring::TTFX_CELL_ASPECT };
                let cands = glyph_light_candidates(
                    &self.geom.instances,
                    &self.geom.emits,
                    glyph_look.cell_w,
                    cell_h,
                    self.glyph_grid.rows(),
                );
                let lights = if s.restir[0] != 0.0 {
                    cands
                } else {
                    brightest_glyph_lights(cands, s.manylight[3].max(0.0) as usize)
                };
                for l in lights {
                    self.meta_nodes.push(render::MetaNode {
                        pos: [l.pos.x, l.pos.y, l.pos.z, radius],
                        color: [l.radiance.x, l.radiance.y, l.radiance.z, 1.0],
                    });
                }
            } else {
                for (m, t) in src_inst.iter().zip(src_tint.iter()) {
                    let p = m.w_axis.truncate();
                    let c = if palette_active {
                        Vec3::new(t.x, t.y, t.z)
                    } else {
                        (p - bounds.min) / span
                    };
                    self.meta_nodes.push(render::MetaNode {
                        pos: [p.x, p.y, p.z, radius],
                        color: [c.x, c.y, c.z, 1.0],
                    });
                }
            }
        }
        // Field Volume (#348): the density-cloud source selector for SurfaceMode::Volume.
        //   fieldvol = [source, smooth, exposure_db, calibrate, gain, _, _, _]
        //   source: 0 Legacy · 1 Auto · 2 FieldBaked · 3 SmoothedNode.
        // Legacy (0, the DEFAULT) is byte-identical: no smoothing widen, no bake, and
        // the exposure/gain/calibrate multiplier collapses to 1.0 at defaults. Only the
        // opt-in sources change the bake; the exposure hook is inert until touched.
        let fv_source = organon_core::params::FieldVolSource::from_u32(s.fieldvol[0] as u32);
        // Auto dispatches by generator: the field generators (Maxwell/Acoustic) get the
        // analytic field-energy bake, everything else gets the smoothed node bake.
        let fv_is_field_gen =
            generator == GeneratorMode::MaxwellField || generator == GeneratorMode::Acoustic;
        let want_field_baked = matches!(fv_source, organon_core::params::FieldVolSource::FieldBaked)
            || (matches!(fv_source, organon_core::params::FieldVolSource::Auto) && fv_is_field_gen);
        // A node generator has no analytic field to bake, so FieldBaked there can't do a
        // true energy bake — give it the smoothed-node CLOUD (the closest thing) rather
        // than silently falling back to the Legacy scraggle (Bugbot). SmoothedNode always
        // smooths; Auto smooths every non-field generator.
        let want_smoothed_node = matches!(fv_source, organon_core::params::FieldVolSource::SmoothedNode)
            || (matches!(
                fv_source,
                organon_core::params::FieldVolSource::Auto
                    | organon_core::params::FieldVolSource::FieldBaked
            ) && !fv_is_field_gen);
        // The analytic field-energy bake only applies to Volume on a field generator;
        // otherwise fall back to the node bake (byte-identical to Legacy when the
        // smoothing/exposure dials are neutral).
        let field_baked = volume && want_field_baked && fv_is_field_gen;

        // Duo-Field volume (#348/#349 Tier 3): when the field bake is active on a field
        // generator with Calibrated colour, render BOTH channels — Acoustic pressure/
        // velocity, or Maxwell E/B — as two interleaving coloured clouds. This does NOT
        // need audio-multipole (the duo bake handles bands internally). Cavity /
        // non-field / Aesthetic or LUFS colour → the plain single-colour bake.
        let fv_band_drives = audio_band_drives(&s);
        let fv_use_bands = audio_multipole_on(&s) && fv_band_drives.iter().any(|d| *d > 0.0);
        let fv_band_coloured = field_baked
            && match generator {
                GeneratorMode::Acoustic => s.acoustic2[0] <= 0.5, // radiating (not cavity)
                GeneratorMode::MaxwellField => true,
                _ => false,
            }
            && {
                let cm = organon_core::params::ColourMode::from_u32(s.colour[0] as u32);
                let cs = organon_core::params::CalColourSource::from_u32(s.colour[4] as u32);
                matches!(cm, organon_core::params::ColourMode::Calibrated)
                    && matches!(
                        cs,
                        organon_core::params::CalColourSource::Auto
                            | organon_core::params::CalColourSource::Band
                    )
            };

        // Tier-2 exposure/gain/calibrate → a single density/emission multiplier folded
        // into the Volume dials. exposure_db=0, gain=1, calibrate=0 → exactly 1.0, so
        // Legacy (and every neutral Volume look) stays byte-identical.
        let fv_exposure = 10.0f32.powf(s.fieldvol[2] / 20.0); // dB → linear gain
        let fv_gain = s.fieldvol[4];
        let fv_cal = if s.fieldvol[3] != 0.0 {
            // Key the drive to calibrated loudness: `calibrated_drive(LUFS)²`.
            // audiometer[0]=momentary LUFS (unmeasured 0.0 → silence), analytical[2]=floor,
            // analytical[1]=ref/target.
            let d = math::calibrated_drive(momentary_lufs(&s), s.analytical[2], s.analytical[1], 1.0);
            d * d
        } else {
            1.0
        };
        let fv_boost = (fv_exposure * fv_gain * fv_cal).max(0.0);

        // SmoothedNode / Auto-on-node-generator: widen the metaball influence radius
        // (and hence the kernel) by `smooth` so dense clouds read as a soft volume,
        // not the metaball skin. `smooth = 1` (default) is neutral.
        let fv_smooth = if want_smoothed_node { s.fieldvol[1].max(0.0) } else { 1.0 };

        // Metaball (iso) vs Volume (emissive medium) share the field bake. In Volume
        // mode the bake uses the volume radius + a soft field, and the raymarch reads
        // the density/emission/absorption/steps; in Metaball mode the vol_* are 0.
        let meta_params = if volume {
            // Duo-Field: an emissive volume SUMS colour along the ray, so two interleaving
            // colours (pressure orange + velocity blue) add to white/pink and the structure
            // is lost. Render it as NESTED COLOURED SURFACES instead — raise absorption so
            // front shells occlude the ones behind (you see each shell's own colour) and
            // cap emission so the core can't blow out to white. Non-duo Volume unchanged.
            let (vd, ve, va) = if fv_band_coloured {
                (
                    (s.volume[1] * fv_boost).max(1.0), // density up so absorption bites
                    (s.volume[2] * fv_boost).min(1.0), // emission capped (no blow-out)
                    s.volume[3].max(3.0),              // high absorption → surface-like shells
                )
            } else {
                (s.volume[1] * fv_boost, s.volume[2] * fv_boost, s.volume[3])
            };
            render::MetaballParams {
                radius: s.volume[0] * fv_smooth,
                threshold: 0.05, // a low iso so the field stays soft/filled for the medium
                smoothness: 0.0,
                vol_density: vd,
                vol_emission: ve,
                vol_absorption: va,
                steps: s.volume[4],
                band_coloured: if fv_band_coloured { 1.0 } else { 0.0 },
            }
        } else {
            render::MetaballParams {
                radius: s.metaball[0],
                threshold: s.metaball[1],
                smoothness: s.metaball[2],
                vol_density: 0.0,
                vol_emission: 0.0,
                vol_absorption: 0.0,
                steps: 0.0,
                band_coloured: 0.0,
            }
        };
        // Beat → voxel fill threshold: the decaying per-beat envelope (same shape as
        // the pulse routing) pushes the threshold so the block-world swells /
        // dissolves on tempo. Inert when Pulse is off or the amount is 0.
        let vox_beat_env = if s.pulse != 0 {
            (-(self.beat_pos.fract() as f32) * 6.0).exp()
        } else {
            0.0
        };
        // GI cone-march distance is a fraction of the structure size, so it reads the
        // same across generators of very different scale.
        let vox_diag = (bounds.max - bounds.min).length().max(1e-3);
        let voxel_params = render::VoxelParams {
            res: s.voxel[0].clamp(16.0, 256.0) as u32,
            threshold: (s.voxel[1] + s.voxel[8] * vox_beat_env).max(1e-3),
            radius: s.voxel[2],
            emission: s.voxel[4],
            ao: s.voxel[5],
            shadow: s.voxel[6],
            quantize: s.voxel[7],
            sharpness: 1.5,
            gi_on: s.voxel_gi[0] >= 0.5,
            gi_strength: s.voxel_gi[1],
            gi_max_dist: s.voxel_gi[2] * vox_diag,
            gi_sky: s.voxel_gi[3],
        };
        let (meta_min, meta_max) = (bounds.min, bounds.max);

        // Field Volume (#348) — analytic field-energy bake. When the source selects the
        // field bake for a Maxwell/Acoustic Volume, evaluate the field's energy density
        // into a FIELD_RES³ grid over the scene bounds (uploaded straight into the
        // Volume field texture by the renderer, replacing the node point-set metaball
        // bake → no far-node scraggle). Reconstructs the same base point/monopole field
        // the aura traces (band/antenna/cavity refinements are aura-only; the density
        // cloud only needs the representative energy landscape). Empty otherwise.
        self.field_vol_grid.clear();
        // #412 Tier 3 Phase 0: when the FDTD solver is on (Maxwell + Volume), march the
        // CPU Yee grid this frame and fill the volume from its LIVE energy — the field
        // propagates (retardation emergent) instead of being sampled from the closed
        // form. Takes precedence over the analytic bake; freed when off.
        let fdtd_on = generator == GeneratorMode::MaxwellField && volume && s.fdtd[0] > 0.5;
        if !fdtd_on {
            self.fdtd_sim.fdtd = None;
        }
        if fdtd_on {
            self.fdtd_sim.run_fdtd(&s, meta_min, meta_max, self.gen_phase, &mut self.field_vol_grid);
        } else if field_baked && fv_band_coloured {
            // Duo-Field volume (#348/#349 T3): render BOTH channels — the scalar PRESSURE
            // (the acoustic "E") and the vector VELOCITY / circulation (the "B") — as
            // separately-coloured energy clouds, so the volume shows the interleaving
            // internal structure the TUBE view does ("two waves flowing over each other,
            // never touching" — they're ~90° out of phase, so one channel's bright shells
            // sit in the other's nodes), each with its own angular orientation. Pressure
            // takes the LUT's warm end, velocity the cool end (so the LUT still colours the
            // two channels: Turbo → red/blue, Inferno → orange/purple, …). The band-driven
            // source stack + the generator's wavenumber `k` (raise it for more shells) shape
            // the detail, and the total energy breathes with the audio.
            let lut = s.colour[3] as u32;
            let colour_a = math::calibrated_colour(0.85, lut); // channel A (pressure / E) → warm
            let colour_b = math::calibrated_colour(0.12, lut); // channel B (velocity / B) → cool
            if generator == GeneratorMode::MaxwellField {
                // Maxwell E/B duo: blend 0 = E, blend 1 = B. MaxwellBands when audio-
                // multipole is on (the spectrum shapes the lobes), else the point/dipole
                // field with the finite antenna — mirroring the single-colour bake.
                let m = &s.maxwell;
                let fphase = self.maxdip_phase as f32;
                let band_elems = if audio_multipole_on(&s) {
                    audio_band_elems(&s)
                } else {
                    Vec::new()
                };
                let mk = |blend: f32| -> math::AnalyticField {
                    if !band_elems.is_empty() {
                        math::AnalyticField::MaxwellBands {
                            elems: band_elems.clone(),
                            blend,
                            near: m[7],
                            r_min: m[10],
                            phase: fphase,
                        }
                    } else {
                        let dipoles = m[3] > 0.5;
                        let sources = math::maxwell_sources(
                            (m[2] as usize).max(1),
                            m[4],
                            dipoles,
                            m[5],
                            m[6],
                            self.gen_phase as f32,
                        );
                        let antenna_segs = if s.maxenergy[5] != 0.0 { 64 } else { 0 };
                        math::AnalyticField::Maxwell {
                            sources,
                            dipoles,
                            blend,
                            k: m[8],
                            near: m[7],
                            r_min: m[10],
                            phase: fphase,
                            antenna_len: s.maxenergy[4],
                            antenna_segs,
                            drive: audio_dipole_drive(&s),
                            offset: Vec3::ZERO,
                        }
                    }
                };
                self.field_vol_grid = math::bake_duo_field_energy(
                    &mk(0.0), colour_a, &mk(1.0), colour_b, meta_min, meta_max, render::FIELD_RES,
                );
            } else {
                // Acoustic pressure/velocity duo. Spectrum → lobes when audio-multipole is
                // on, else the fixed multipole; the two channels share the source stack and
                // differ only by the pressure↔velocity blend.
                let fphase = self.maxdip_phase as f32;
                let a = &s.acoustic;
                let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                let sources = if fv_use_bands {
                    math::acoustic_band_sources(&fv_band_drives, a[4])
                } else {
                    math::acoustic_sources(math::AcousticKind::from_u32(a[0] as u32), a[4])
                };
                // Band mode carries loudness in each source's q (only the beat pump scales);
                // the fixed-multipole fallback breathes with the broadband RMS drive × pump.
                let duo_drive = if fv_use_bands { pump } else { audio_dipole_drive(&s) * pump };
                let mk = |blend: f32| math::AnalyticField::Acoustic {
                    sources: sources.clone(),
                    blend,
                    k: a[1],
                    near: a[2],
                    r_min: a[5],
                    phase: fphase,
                    drive: duo_drive,
                    intensity: s.acoustic2[6],
                };
                self.field_vol_grid = math::bake_duo_field_energy(
                    &mk(0.0), colour_a, &mk(1.0), colour_b, meta_min, meta_max, render::FIELD_RES,
                );
            }
        } else if field_baked {
            let fphase = self.maxdip_phase as f32;
            let field = match generator {
                GeneratorMode::MaxwellField => {
                    // Mirror the aura/arrows EXACTLY so the density cloud agrees with what
                    // they draw: the band-multipole field when audio-multipole is on (the
                    // spectrum shapes the cloud), else the point/dipole field WITH the
                    // finite-antenna energization when active. (Bugbot #7 — the bake used to
                    // ignore both bands + antenna.) Breathes with the RMS drive.
                    let m = &s.maxwell;
                    let aura_blend = s.maxenergy[7];
                    let band_elems = if audio_multipole_on(&s) {
                        audio_band_elems(&s)
                    } else {
                        Vec::new()
                    };
                    if !band_elems.is_empty() {
                        math::AnalyticField::MaxwellBands {
                            elems: band_elems,
                            blend: aura_blend,
                            near: m[7],
                            r_min: m[10],
                            phase: fphase,
                        }
                    } else {
                        let dipoles = m[3] > 0.5;
                        let sources = math::maxwell_sources(
                            (m[2] as usize).max(1),
                            m[4],
                            dipoles,
                            m[5],
                            m[6],
                            self.gen_phase as f32,
                        );
                        let antenna_segs = if s.maxenergy[5] != 0.0 { 64 } else { 0 };
                        math::AnalyticField::Maxwell {
                            sources,
                            dipoles,
                            blend: aura_blend,
                            k: m[8],
                            near: m[7],
                            r_min: m[10],
                            phase: fphase,
                            antenna_len: s.maxenergy[4],
                            antenna_segs,
                            drive: audio_dipole_drive(&s),
                            offset: Vec3::ZERO,
                        }
                    }
                }
                _ => {
                    // Acoustic. Mirror the geometry/aura EXACTLY so the density cloud
                    // agrees with the visible nodes + motes: Cavity (Chladni) standing-
                    // wave modes when the model is Cavity, else the radiating multipole.
                    let a = &s.acoustic;
                    let a2 = &s.acoustic2;
                    let a3 = &s.acoustic3;
                    let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                    let intensity = a2[6];
                    if a2[0] > 0.5 {
                        // Cavity model (#325 T4): the standing-wave eigenmode whose
                        // pressure nodal planes are the 3-D Chladni figures. Mirror the
                        // aura's morphed/tweened modes + per-axis audio breathe so the
                        // cloud matches (Bugbot: the bake previously ignored the model).
                        let base_modes = Vec3::new(a2[1], a2[2], a2[3]);
                        let mut modes = math::cavity_morph_modes_tween(base_modes, self.beat_pos, a2[4], a3[0]);
                        modes += Vec3::new(a3[1], a3[2], a3[3]) * cavity_audio_breathe(&s);
                        let dims = Vec3::splat(a2[5].max(1.0e-3));
                        math::AnalyticField::AcousticCavity {
                            modes,
                            dims,
                            blend: a[14],
                            phase: fphase,
                            drive: audio_dipole_drive(&s) * pump,
                            intensity,
                        }
                    } else {
                        // Radiating multipole: when audio-multipole is on, a band-weighted
                        // monopole stack (each FFT band → its own lobes) replaces the fixed
                        // source, so bass vs treble push different lobes and the shells
                        // reshape with the music (#348 Tier-3 spatial spectrum, pulled into
                        // the volume). Else the fixed multipole, breathing by loudness.
                        let band_drives = audio_band_drives(&s);
                        let use_bands = audio_multipole_on(&s) && band_drives.iter().any(|d| *d > 0.0);
                        let sources = if use_bands {
                            math::acoustic_band_sources(&band_drives, a[4])
                        } else {
                            let kind = math::AcousticKind::from_u32(a[0] as u32);
                            math::acoustic_sources(kind, a[4])
                        };
                        // Band mode bakes loudness into each source's q already, so only the
                        // beat pump scales it (else energy ~ drive² on top of band amplitudes);
                        // the static fallback uses the broadband RMS drive × pump.
                        let drive = if use_bands { pump } else { audio_dipole_drive(&s) * pump };
                        math::AnalyticField::Acoustic {
                            sources,
                            blend: a[14],
                            k: a[1],
                            near: a[2],
                            r_min: a[5],
                            phase: fphase,
                            drive,
                            intensity,
                        }
                    }
                }
            };
            self.field_vol_grid = math::bake_field_energy(
                &field,
                meta_min,
                meta_max,
                render::FIELD_RES,
                1.0,
            );
        }

        // Mandelbulb params: spin + morph ride the generic gen_phase (so they
        // advance with global Speed and the Speed Pulse → beat-reactive), scale
        // breathes with the pulse, and the colour cycle phase flows the trap
        // banding. Centre 0 (the bulb is built about the origin).
        let mb = &s.mandelbulb;
        let mandel_params = render::MandelParams {
            power: mb[0],
            iterations: mb[1].max(1.0) as u32,
            scale: mb[2].max(1e-3) * breath_scale.x,
            steps: mb[3].max(8.0) as u32,
            spin_angle: (self.gen_phase * mb[4] as f64) as f32,
            morph_angle: (self.gen_phase * mb[5] as f64) as f32,
            color: mb[6],
            bailout: mb[7],
            color_phase: self.color_phase as f32,
            center: Vec3::ZERO,
        };

        // Creature Engine params (#476 Tier 1). The body plan is built CPU-side
        // from `form` (deterministic); the travelling peristaltic swim phase
        // advances off the global Speed clock (so it rides the beat via Speed
        // Pulse, like Mandelbulb's spin/morph). The world size breathes with the
        // universal Breath. `creature_plan` must outlive the `render()` call below
        // (it's borrowed by `CreatureParams`).
        let cr = &s.creature;
        let cr2 = &s.creature2;
        let cr3 = &s.creature3;
        let creature_form = cr[0].max(0.0) as u32;
        // Tier 2b: a loaded JSON body plan (from the "Load Creature…" sidecar) wins
        // over the built-in `form` plan; else fall back to the hand-authored form.
        // Cloned to a local (cheap, ~30 small Copy prims) so no borrow of `self` is
        // held across the later render call.
        let creature_plan = match self.creature_loaded.as_ref() {
            Some(p) => p.clone(),
            None => math::creature_body_plan(creature_form),
        };
        let creature_bound = math::creature_bounds(&creature_plan);
        let creature_params = render::CreatureParams {
            prims: &creature_plan,
            scale: cr[1].max(1e-3) * breath_scale.x,
            steps: cr[2].max(8.0) as u32,
            swim_phase: (self.gen_phase * cr[3] as f64) as f32,
            warp_amp: cr[4],
            warp_freq: cr[5],
            rim: cr[6],
            glow_scale: cr[7],
            bound: creature_bound,
            // Tier 2a metachronal wave: the band's phase advances off the global
            // Speed clock (rides the beat via Speed Pulse, like the swim).
            wave_freq: cr2[1],
            wave_phase: (self.gen_phase * cr2[0] as f64) as f32,
            wave_sharp: cr2[2],
            wave_amount: cr2[3],
            // Tier 2c anatomy overlay.
            overlay_on: cr3[0] > 0.5,
            overlay_opacity: cr3[1],
            overlay_bright: cr3[2],
            // Colour palette / spectrum: the same Palette selector every generator
            // uses (surface_fx[6]; 0 = Native → the bioluminescent blue).
            palette: s.surface_fx[6] as u32,
        };

        // Neural-field params (#200 Tier 1). The network identity + manual walk +
        // omega ride `s.neural[..]` (Tier 0); the raymarch/field dials ride
        // `s.neural2[..]`. Beat-driven latent walk: a triangle-wave morph A→B whose
        // rate is walk-cycles per beat off the PLL beat clock (0 = static). The
        // world size breathes with the universal Breath; `time` slowly animates the
        // field's own 4th input.
        let nn = &s.neural;
        let nn2 = &s.neural2;
        let neural_field_params = render::NeuralFieldParams {
            seed_a: nn[1].max(0.0) as u32,
            seed_b: nn[2].max(0.0) as u32,
            walk: neural_walk_resolved,
            omega: nn[4].max(1e-3),
            scale: nn2[0].max(1e-3) * breath_scale.x,
            coord: nn2[1].max(1e-3),
            iso: nn2[2],
            steps: nn2[3].max(8.0) as u32,
            march: nn2[4].max(0.0),
            color: nn2[5].clamp(0.0, 1.0),
            time: (self.gen_phase * 0.5) as f32,
            center: Vec3::ZERO,
        };

        // Minimal-surface params (#127): the isolevel breathes the channels on the
        // beat (the same decaying per-beat envelope as the voxel/pulse system, only
        // while Pulse is on); the world size breathes with the universal Breath; the
        // colour cycle flows the channel banding. Twist is a static helical shear of
        // the domain (a shape lever); the camera orbit supplies the motion. Centre 0
        // (the surface is built about the origin).
        let ms = &s.minimal_surface;
        let ms_beat = if s.pulse != 0 {
            (-(self.beat_pos.fract() as f32) * 6.0).exp()
        } else {
            0.0
        };
        let minimal_params = render::MinimalParams {
            family: ms[0] as u32,
            scale: ms[1].max(1e-3) * breath_scale.x,
            cells: ms[2].max(0.1),
            iso: ms[3] + ms[8] * ms_beat,
            thickness: ms[4].max(0.0),
            twist: ms[5],
            steps: ms[6].max(24.0) as u32,
            color: ms[7],
            color_phase: self.color_phase as f32,
            center: Vec3::ZERO,
        };

        // Lens params (#258 Tier 3): the world size breathes with the universal
        // Breath; focal / aperture / thickness / plano are static shape levers (the
        // camera orbit supplies the motion). Centre 0 (built about the origin).
        let ln = &s.lens;
        let lens_params = render::LensParams {
            focal: ln[0].max(0.05),
            aperture: ln[1].max(0.02),
            thickness: ln[2].max(0.01),
            plano: ln[3] > 0.5,
            scale: ln[4].max(1e-3) * breath_scale.x,
            steps: ln[5].max(16.0) as u32,
            center: Vec3::ZERO,
        };

        // Kaleidoscopic Fractal params: rotation + breathe + tunnel-flow ride the
        // generic gen_phase (so they advance with global Speed and the Speed Pulse
        // → beat-reactive). `time` is the raw accumulated phase; the shader applies
        // spin/breathe to it.
        let kf = &s.kifs;
        // The E8 Petrie projection (8 rings × 30 roots) is fixed geometry — compute
        // it once via the unit-tested CPU projection and reuse every frame.
        static E8_RINGS: std::sync::OnceLock<[f32; 16]> = std::sync::OnceLock::new();
        let e8_rings = *E8_RINGS.get_or_init(|| {
            let r = math::e8_petrie_rings();
            let mut out = [0.0f32; 16];
            for k in 0..8 {
                out[2 * k] = r[k][0];
                out[2 * k + 1] = r[k][1];
            }
            out
        });
        // 8-D plane rotation: when `kf[24]` (E8 flow) > 0, tumble the 2-plane through
        // 8-space and re-project all 240 roots this frame. Basis (roots + Coxeter
        // plane u,v + complement w1,w2) is cached; the projection is ~4k mults/frame.
        struct E8Basis {
            roots: Vec<[f64; 8]>,
            u: [f64; 8],
            v: [f64; 8],
            w1: [f64; 8],
            w2: [f64; 8],
        }
        static E8_BASIS: std::sync::OnceLock<E8Basis> = std::sync::OnceLock::new();
        let e8b = E8_BASIS.get_or_init(|| {
            let (u, v) = math::e8_coxeter_plane();
            let (w1, w2) = math::e8_complement(&u, &v);
            E8Basis { roots: math::e8_roots(), u, v, w1, w2 }
        });
        let e8_flow = kf[24] as f64;
        let ang = self.gen_phase * e8_flow;
        let (c1, s1) = (ang.cos(), ang.sin());
        let (c2, s2) = ((ang * 0.6180339887).cos(), (ang * 0.6180339887).sin());
        let inv = 1.0 / 2.0f64.sqrt(); // root norm = √2 → unit disk
        let mut e8_points = [[0.0f32; 2]; 240];
        for (k, r) in e8b.roots.iter().enumerate() {
            let mut x = 0.0;
            let mut y = 0.0;
            for i in 0..8 {
                x += r[i] * (c1 * e8b.u[i] + s1 * e8b.w1[i]);
                y += r[i] * (c2 * e8b.v[i] + s2 * e8b.w2[i]);
            }
            e8_points[k] = [(x * inv) as f32, (y * inv) as f32];
        }
        // 3-D E8 shadow (Lattice mode): project each root onto three orthonormal
        // 8-vectors — the Coxeter plane (u, v) plus a depth axis that tumbles in the
        // w1–w2 plane (so the shadow turns when E8 flow is on, static otherwise).
        let (cz, sz) = (ang.cos(), ang.sin());
        let mut e8_points3 = [[0.0f32; 3]; 240];
        for (k, r) in e8b.roots.iter().enumerate() {
            let mut x = 0.0;
            let mut y = 0.0;
            let mut z = 0.0;
            for i in 0..8 {
                x += r[i] * e8b.u[i];
                y += r[i] * e8b.v[i];
                z += r[i] * (cz * e8b.w1[i] + sz * e8b.w2[i]);
            }
            e8_points3[k] = [(x * inv) as f32, (y * inv) as f32, (z * inv) as f32];
        }
        let kifs_params = render::KifsParams {
            time: self.gen_phase as f32,
            sectors: kf[0],
            fold: kf[1],
            iterations: kf[2],
            iter_rot: kf[3],
            spin: kf[4],
            breathe: kf[5],
            zoom: kf[6],
            tunnel: kf[7],
            rays: kf[8],
            ring: kf[9],
            glow: kf[10],
            hue: kf[11],
            pattern: kf[12],
            palette: kf[13],
            color_speed: kf[14],
            warp: kf[15],
            flow: kf[16],
            churn: kf[17],
            petals: kf[18],
            contrast: kf[19],
            sharp: kf[20],
            invert: kf[21],
            dispersion: kf[22],
            space: kf[23],
            view: kf[29],
            relief: kf[25],
            relief_elev: kf[26],
            relief_steps: kf[27],
            relief_shine: kf[28],
            e8_rings,
            e8_flow: kf[24],
            e8_points,
            e8_points3,
        };

        // Scene Kaleidoscope (#361 Tier 1): a post-stage fold of the resolved HDR
        // scene. The fold rotation rides the animation clock (`gen_phase`) so spin
        // advances with global Speed + the beat, like the KIFS field.
        let kl = &s.kaleido;
        let kaleido_params = render::KaleidoParams {
            enabled: kl[0] != 0.0,
            sectors: kl[1],
            mode: kl[2],
            angle: kl[3] * self.gen_phase as f32 + kl[4] * std::f32::consts::TAU,
            zoom: kl[5],
            center: [kl[6], kl[7]],
            mix: kl[8],
            twist: kl[9],
            tint_hue: kl[10],
            tint_amt: kl[11],
            seam: kl[12],
        };

        // Reaction–diffusion params (diffusion rates + dt fixed for stability; the
        // editor exposes feed/kill/scale/intensity/pigment).
        let rd_params = render::RdParams {
            feed: s.rd[0],
            kill: s.rd[1],
            diffuse_u: 0.16,
            diffuse_v: 0.08,
            dt: 1.0,
            scale: s.rd[2],
            intensity: s.rd[3],
            albedo_mix: s.rd[4],
        };

        let Some(gfx) = self.gfx.as_mut() else { return };

        // Terrain backdrop: re-synthesize + re-upload the 256² noise tile when the
        // editor changes the noise type or seed (cheap; only on a change).
        let t_key = (s.terrain[9] as u32, s.terrain[10] as u32);
        if t_key != self.terrain_key {
            self.terrain_key = t_key;
            self.terrain_noise = render::terrain_gen_noise(t_key.0, t_key.1);
            gfx.renderer.set_terrain_noise(&gfx.device, &gfx.queue, &self.terrain_noise);
        }

        // --- The world/window seam (#541 S2 T3 → #572 stage 3) ---------------
        // WHERE this frame goes, before anything reads a size or a format. Since stage 3 the
        // caller states it outright — a swapchain image it already acquired, or a texture it
        // owns — so there is no surface to interrogate. `out.presented` remains the one bit the
        // rest of the frame consults to skip display-only behaviour.
        let out = FrameOutput::of(target.size, target.format, target.presented);
        let size = out.size;
        // Bound here because `out` is shadowed further down (`if let Some(out) = cap_eff`), and
        // the recorder's feed site sits inside that shadow — where `out.presented` silently
        // means something else entirely. See the recorder gating note (#582).
        let presented = out.presented;
        // (#572 stage 3) The display's EDR headroom is the host's measurement, delivered per
        // frame. Latched into the field the rest of the frame (and the HUD, and the feedback
        // channel) already read, so exactly one line changed rather than a dozen call sites.
        // Offscreen frames leave it alone: they are composited SDR regardless, and clobbering
        // it would make the window's own readouts lie on every mirror frame.
        if target.presented {
            self.hdr.hdr_max = target.hdr_max;
        }
        // The frame's output colour format (this used to read `gfx.config.format`
        // directly — i.e. the swapchain's). The composite / FX / temporal passes
        // are built for exactly one format, so rebuild them when it changes. On
        // the window path `set_hdr` already did this when it flipped the
        // swapchain, so this is a no-op there; it fires for an offscreen target
        // whose texture format differs from the last frame's output.
        let out_format = out.format;
        if out_format != gfx.out_format {
            gfx.renderer.set_surface_format(&gfx.device, out_format);
            gfx.out_format = out_format;
        }

        // --- Capture / production frame (#135 Phase 1) -----------------------
        // `cap_out` = the fixed output size (None = Native → render straight to the
        // window). When set, the scene renders into an offscreen texture of exactly
        // that size (so the projection aspect + render dims match the output), then a
        // letterbox blit centres it in the window. Edge-detect the editor's
        // frame-guide flag so the 'G' key can still toggle it; honour "lock window".
        //
        // long_edge = 0 means "match the display": use the window's longest side, so
        // picking an aspect reframes at full native resolution (a 4K display stays 4K)
        // instead of downscaling to a fixed 1080p-class signal.
        let long_edge = match s.capture[1] as u32 {
            0 => size.0.max(size.1),
            v => v,
        };
        let cap_out = capture::production_size(
            s.capture[0] as u32,
            long_edge,
            s.capture[2] as u32,
            s.capture[3] as u32,
        );
        let render_size = cap_out.unwrap_or(size);
        let cap_backdrop = [s.capture[4], s.capture[5], s.capture[6]];
        let guide_ipc = s.capture[7] != 0.0;
        if guide_ipc != self.last_guide_ipc {
            self.last_guide_ipc = guide_ipc;
            self.frame_guide = guide_ipc;
        }
        // Lock window to output: re-request the inner size whenever the *actual*
        // window diverges from the production output — not just on a setting change —
        // so it self-corrects after a manual/OS resize or an ignored request (a
        // converging request, since matching the size makes the test stop firing).
        // (Window targets only — there is no OS window to resize behind an
        // offscreen target; its size is the caller's business.)
        if s.capture[8] != 0.0 && out.presented {
            if let Some(o) = cap_out {
                if size != o {
                    requests.inner_size = Some(o);
                }
            }
        }

        // In-app recorder (#430 Tier 0): consume the 'R' toggle, then honour the N-bar
        // beat-synced auto-stop. Start captures `beat_pos` so `record_done` measures the
        // musical length off the same clock everything else rides. The output size is the
        // production size (or the window in Native); the format tracks the HDR toggle.
        //
        // **Every site below is gated on `out.presented` (#582).** A take belongs to the
        // *presented* window: its size, its format and its production texture. The frame mirror
        // (#554 T1) renders a second, offscreen 640×360 `Rgba8UnormSrgb` pass through this same
        // function whenever an editor is open, and an ungated recorder treats that pass as if it
        // were the window — which latched the wrong dimensions on start, fed mirror-sized frames
        // mid-take, and, because `matches()` then disagreed with the very next window frame,
        // ended every take after 2–3 frames. Measured on the #582 Mac pass:
        //   `take((1100,760), Rgba16Float) vs frame((640,360), Rgba8UnormSrgb)`
        // and deleting the stale mirror flag took the same take from 1 frame to 483.
        //
        // Pre-existing, not a stage-3 regression: `main` at fe27310 has no gate here either, so
        // this has been latent since the mirror landed and only needed an editor open to bite.
        let record = &mut self.record;
        if out.presented && record.toggle_pending {
            record.toggle_pending = false;
            if let Some(rec) = record.recorder.take() {
                // #452 Tier 3: reply to a CLI `record stop` with the finished file path
                // (grabbed before `finish` consumes the recorder).
                let path = rec.out_path().display().to_string();
                record.pending_finalizers.push(rec.finish_async(&gfx.device));
                record.fixed = false;
                // A manual stop also leaves chunk mode — otherwise the roll below would
                // immediately open a new clip and 'R' would look like it did nothing.
                record.chunk_armed = false;
                if let Some((nonce, false)) = self.cmd_chan.eyes_record_pending.take() {
                    append_eyes_reply(&nonce, &Ok(path));
                }
            } else {
                let out = cap_out.unwrap_or(size);
                let perfect = record.perfect_pending;
                match recorder::Recorder::start(
                    &gfx.device,
                    out,
                    out_format,
                    self.hdr.hdr_enabled,
                    self.hdr.hdr_wide,
                    recorder::TakeOpts {
                        fps: record.fps,
                        perfect,
                        ..Default::default()
                    },
                ) {
                    Ok(rec) => {
                        record.start_beat = self.beat_pos;
                        // #452 Tier 3: reply to a CLI `record start` with the path the
                        // file WILL be written to (known once the recorder exists).
                        if let Some((nonce, true)) = self.cmd_chan.eyes_record_pending.take() {
                            append_eyes_reply(&nonce, &Ok(rec.out_path().display().to_string()));
                        }
                        record.recorder = Some(rec);
                        record.fixed = perfect;
                        record.error = None;
                    }
                    Err(e) => {
                        // start() logs to stderr, which is LOST when the plugin spawns the
                        // visual — so surface it on screen or the failure is invisible.
                        // Revert the fixed-clock engage from the top of render() — with no
                        // recorder, the animation must not stay on the fixed step.
                        record.fixed = false;
                        if let Some((nonce, _)) = self.cmd_chan.eyes_record_pending.take() {
                            append_eyes_reply(&nonce, &Err(format!("recorder start failed: {e}")));
                        }
                        record.error = Some((
                            format!("REC failed: {e} (install ffmpeg or set $ORGANON_FFMPEG)"),
                            std::time::Instant::now(),
                        ));
                    }
                }
            }
        }

        // #430 chunk mode: consume the 'C' arm/disarm request. Handled here, not in the key
        // handler, because laying out the grid needs the live snapshot — the host's phase,
        // the tempo, and the meter.
        if out.presented && record.chunk_arm_pending {
            record.chunk_arm_pending = false;
            if record.chunk_armed {
                // Disarm: the in-flight clip is a partial phrase, so throw it away rather
                // than leave a short file among the aligned ones.
                record.chunk_armed = false;
                if let Some(rec) = record.recorder.take() {
                    record.pending_finalizers.push(rec.discard(&gfx.device));
                }
                record.note = Some((
                    format!("Chunk record OFF — {} clip(s) written", record.chunk_index),
                    std::time::Instant::now(),
                ));
            } else if record.recorder.is_none() {
                let phrase = record.chunk_phrase_beats.max(1.0);
                let bpm = active_bpm(&s).max(1.0) as f64;
                // Is the host actually giving us a musical grid to align to? (Ableton hands
                // an audio-effect `pos_beats` only sometimes — see `advance_beat_clock`.)
                let host_live = s.transport[0] > 0.5 && s.transport[3] > 0.5 && s.tempo_sync != 0;
                let host_beats = s.transport[1] as f64;
                // Phase-align our continuous `beat_pos` grid to the host's absolute phrase
                // grid, so clip boundaries land on the arrangement's phrases and not on an
                // arbitrary offset from wherever the visual happened to start.
                let offset = if host_live {
                    self.beat_pos - host_beats.rem_euclid(phrase)
                } else {
                    self.beat_pos
                };
                // Strictly the NEXT boundary, so there is always a count-in — even if 'C' is
                // pressed a hair after a downbeat.
                let first = recorder::next_boundary(self.beat_pos, phrase, offset);
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() % 1_000_000)
                    .unwrap_or(0);
                record.chunk_grid_offset = first;
                record.chunk_index = 0;
                record.chunk_bpm = bpm;
                record.chunk_bar = host_live.then(|| {
                    ((host_beats + (first - self.beat_pos)) / beats_per_bar).floor() as u64 + 1
                });
                record.chunk_session = Some(format!("Organon-{:.0}bpm-{stamp}", bpm));
                record.chunk_armed = true;
                // Chunk mode rides the host's real-time transport; the fixed-timestep clock
                // deliberately decouples from it, so the two are mutually exclusive.
                record.fixed = false;
                let quota = recorder::chunk_frames(0, phrase, bpm, record.fps.value());
                record.note = Some((
                    format!(
                        "Chunk record ARMED — {:.0}-beat clips, {} frames each @ {}",
                        phrase,
                        quota,
                        record.fps.label()
                    ),
                    std::time::Instant::now(),
                ));
            }
        }

        // --- #430 phrase chunk mode: roll to the next clip on the boundary ---------------
        // (Presented frames only — see the recorder note above; a mirror pass must not roll a
        // clip, and `chunk_frames` paces off the beat clock, not off how many passes ran.)
        //
        // The take never stops; the *file* does. When the armed clip has emitted its exact
        // frame quota we hand it to the background finalizer and immediately open the next
        // one, warm and gated, so a continuous pass over a song comes out as a folder of
        // grid-aligned clips that butt-join on an NLE timeline.
        if out.presented && record.chunk_armed {
            let phrase = record.chunk_phrase_beats.max(1.0);
            let fps = record.fps.value();
            // The current clip's boundary, computed absolutely (never accumulated).
            let anchor = record.chunk_grid_offset + record.chunk_index as f64 * phrase;

            // 1. Open the shutter once the beat clock reaches this clip's boundary. The
            //    encoder is already warm, so the first frame through the gate really is the
            //    downbeat frame.
            let opened = match record.recorder.as_mut() {
                Some(rec) if rec.is_gated() && self.beat_pos >= anchor => {
                    rec.open_gate();
                    true
                }
                _ => false,
            };
            if opened {
                record.start_beat = anchor;
            }

            // 2. Roll when the quota is met. `finish_async` keeps the multi-second ffmpeg
            //    wait + audio mux off this thread — the whole reason chunking is possible.
            let rolled = record.recorder.as_ref().is_some_and(|r| r.target_reached());
            if rolled {
                if let Some(rec) = record.recorder.take() {
                    record.pending_finalizers.push(rec.finish_async(&gfx.device));
                }
                record.chunk_index += 1;
                if let Some(bar) = record.chunk_bar.as_mut() {
                    // Bars advance with the clips, so clip N's name keeps naming its own bar.
                    *bar += (phrase / beats_per_bar).round().max(1.0) as u64;
                }
            }

            // 3. Make sure a clip is open (the first one, or the successor to a roll).
            if record.recorder.is_none() {
                let out = cap_out.unwrap_or(size);
                let quota = recorder::chunk_frames(record.chunk_index, phrase, record.chunk_bpm, fps);
                let session = record.chunk_session.clone().unwrap_or_else(|| "Organon".to_string());
                let label = match record.chunk_bar {
                    Some(bar) => format!("{session}-c{:03}-bar{:03}", record.chunk_index + 1, bar),
                    None => format!("{session}-c{:03}", record.chunk_index + 1),
                };
                match recorder::Recorder::start(
                    &gfx.device,
                    out,
                    out_format,
                    self.hdr.hdr_enabled,
                    self.hdr.hdr_wide,
                    recorder::TakeOpts {
                        fps: record.fps,
                        // Chunk mode is real-time by construction: it rides the host's
                        // transport, which perfect/fixed-timestep capture deliberately
                        // decouples from. The two can't both be true.
                        perfect: false,
                        gated: true,
                        target_frames: Some(quota),
                        label: Some(label),
                    },
                ) {
                    Ok(rec) => {
                        record.recorder = Some(rec);
                        record.fixed = false;
                        record.error = None;
                    }
                    Err(e) => {
                        // Can't open a clip — leave the mode rather than spin retrying a
                        // failing ffmpeg spawn once per frame.
                        record.chunk_armed = false;
                        record.error = Some((
                            format!("REC failed: {e} (install ffmpeg or set $ORGANON_FFMPEG)"),
                            std::time::Instant::now(),
                        ));
                    }
                }
            }
        }

        let stop_recording = if let Some(rec) = record.recorder.as_ref() {
            let take_out = cap_out.unwrap_or(size);
            // Chunk mode ends its clips on the frame quota, not the N-bar rule.
            let done = !record.chunk_armed
                && recorder::record_done(
                    record.start_beat,
                    self.beat_pos,
                    record.bars,
                    beats_per_bar,
                );
            let agrees = rec.matches(take_out, out_format, self.hdr.hdr_wide);
            if presented && !agrees {
                // Kept past its diagnostic purpose (#582): now that mirror passes are gated
                // out, this only fires on a REAL invalidating change — the aspect, the HDR
                // format or the gamut moved mid-take — which is something the operator wants
                // said out loud rather than discovering in a short file. Once per take.
                eprintln!(
                    "[rec] auto-stop: take({:?}, {:?}, wide={}) vs frame({:?}, {:?}, wide={})",
                    rec.take_size(),
                    rec.take_format(),
                    rec.take_wide(),
                    (take_out.0.max(2), take_out.1.max(2)),
                    out_format,
                    self.hdr.hdr_wide,
                );
            }
            take_should_stop(presented, done, agrees)
        } else {
            false
        };
        if stop_recording {
            if let Some(rec) = record.recorder.take() {
                record.pending_finalizers.push(rec.finish_async(&gfx.device));
            }
            record.fixed = false;
            // An invalidating change (aspect / HDR / gamut switched mid-take) also drops out
            // of chunk mode: the remaining clips would not match the ones already written.
            record.chunk_armed = false;
        }

        // Reap finished background finalizers so the vec doesn't grow across a long session.
        record.pending_finalizers.retain(|h| !h.is_finished());

        // #430 Tier 0: the live on-screen REC line (drawn on the window in render(), never
        // baked into the file). Built here because the beat clock + bar length are in scope.
        let perfect = record.fixed;
        let chunk = record.chunk_armed;
        let phrase = record.chunk_phrase_beats.max(1.0);
        let chunk_index = record.chunk_index;
        let chunk_anchor = record.chunk_grid_offset + chunk_index as f64 * phrase;
        record.hud = record.recorder.as_ref().map(|rec| {
            if chunk {
                if rec.is_gated() {
                    // Counting in: how far to the downbeat that starts clip 1.
                    let togo = (chunk_anchor - self.beat_pos).max(0.0);
                    format!("○ ARMED   clip {} in {:.1} beats", chunk_index + 1, togo)
                } else {
                    // Frames against the clip's exact quota, so an off-by-one cut is visible
                    // on screen rather than only discoverable later in the NLE.
                    let quota =
                        recorder::chunk_frames(chunk_index, phrase, record.chunk_bpm, record.fps.value());
                    format!(
                        "● REC   clip {}   frame {} / {}",
                        chunk_index + 1,
                        rec.frames_emitted(),
                        quota
                    )
                }
            } else {
                let bars = ((self.beat_pos - record.start_beat) / beats_per_bar).max(0.0);
                let mode = if perfect { "  [perfect]" } else { "" };
                if record.bars > 0 {
                    format!("● REC   bar {:.1} / {}{}", bars, record.bars, mode)
                } else {
                    format!("● REC   bar {bars:.1}{mode}")
                }
            }
        });

        // Overlay (#135 P2): editor master toggle edge-detected so the 'T' key keeps
        // its local state; the string sidecar (handle / title) re-read on a gen bump.
        let overlay_ipc = s.overlay[0] != 0.0;
        if overlay_ipc != self.last_overlay_ipc {
            self.last_overlay_ipc = overlay_ipc;
            self.overlay_on = overlay_ipc;
        }
        if s.overlay_gen != self.last_overlay_gen {
            self.last_overlay_gen = s.overlay_gen;
            if let Ok(txt) = std::fs::read_to_string(ipc::overlay_sidecar_path()) {
                // Tiny hand-parsed JSON: { "handle": "...", "title": "..." }.
                self.overlay_handle = json_field(&txt, "handle").unwrap_or_default();
                let t = json_field(&txt, "title").unwrap_or_default();
                self.overlay_title = if t.is_empty() { None } else { Some(t) };
            }
        }

        // Auto path offset, added on top of the manual drag/scroll orbit.
        // Demo (#288): with "fixed camera" on, gate the auto-orbit off so the
        // canonical front-on reference framing holds (drag still works).
        let demo_static = generator == GeneratorMode::Demo && s.demo[3] >= 0.5;
        // Glide length (bars) shared by the storyboard + sequencer (Cut = instant).
        let trans_bars = if s.cam_seq[3] > 0.5 {
            0.0 // Cut
        } else {
            s.cam_clock[2] as f64 // Glide length in bars
        };
        let off: CamOffset = if demo_static {
            CamOffset::default()
        } else if s.cam_story[0] > 0.5 {
            // Storyboard on (highest priority): crossfade the outgoing shot's move
            // (with its framing radius) into the incoming one.
            let g = smoothstep(0.0, 1.0, self.story.glide_t(self.cam_bar_pos, trans_bars));
            let mut a = camera_path_offset(
                story_shot_path(&s.cam_story, self.story.prev),
                self.cam_phase,
                s.cam_amount,
            );
            a.dist *= story_shot_radius(&s.cam_story, self.story.prev);
            let mut b = camera_path_offset(
                story_shot_path(&s.cam_story, self.story.cur),
                self.cam_phase,
                s.cam_amount,
            );
            b.dist *= story_shot_radius(&s.cam_story, self.story.cur);
            let story_off = CamOffset::mix(a, b, g);
            // Blend (#307): like the sequencer, the base orbit-cam (`camera[0]` path)
            // stays effective and `seq_mix` blends the storyboard shot on top — 0 =
            // fully orbit-cam, 1 = fully storyboard.
            let base = camera_path_offset(s.camera[0] as u32, self.cam_phase, s.cam_amount);
            let mix = s.cam_frame[5].clamp(0.0, 1.0);
            CamOffset::mix(base, story_off, mix)
        } else if s.cam_seq[0] > 0.5 {
            // Sequencer on: crossfade the outgoing move into the incoming one over
            // the glide window (Cut = an instant swap on the downbeat).
            let g = smoothstep(0.0, 1.0, self.seq.glide_t(self.cam_bar_pos, trans_bars));
            let a = camera_path_offset(self.seq.prev, self.cam_phase, s.cam_amount);
            let b = camera_path_offset(self.seq.cur, self.cam_phase, s.cam_amount);
            let seq_off = CamOffset::mix(a, b, g);
            // Blend (#307): the base orbit-cam (`camera[0]` path) is ALWAYS effective;
            // `seq_mix` blends the sequencer's move on top. 0 = fully orbit-cam
            // (organic-math), 1 = fully sequencer. Both ride the same `cam_phase`, so
            // the flow-speed dial governs either end.
            let base = camera_path_offset(s.camera[0] as u32, self.cam_phase, s.cam_amount);
            let mix = s.cam_frame[5].clamp(0.0, 1.0);
            CamOffset::mix(base, seq_off, mix)
        } else {
            camera_path_offset(s.camera[0] as u32, self.cam_phase, s.cam_amount)
        };
        // Decoupled dolly (#307): an in/out radius breath on its own bar period,
        // independent of the orbit speed. Depth 0 → factor 1 (inert).
        let dolly = if demo_static {
            1.0
        } else {
            dolly_factor(
                s.cam_dolly[0] as f64,
                s.cam_dolly[1],
                s.cam_dolly[2] as u32,
                self.cam_bar_pos,
            )
        };
        let yaw = self.yaw + off.dyaw;
        let pitch = (self.pitch + off.dpitch)
            .clamp(-scene_input::PITCH_LIMIT, scene_input::PITCH_LIMIT);
        let auto_dist = off.dist * dolly; // the auto in/out factor (for dolly-zoom)
        let distance = self.distance * auto_dist;
        // Camera roll / dutch (#307 Tier 2): the global param + the move's own roll.
        let cam_roll = s.cam_frame[0].to_radians() + off.roll;
        // FOV (#307 Tier 2): base × the move's fov_mul × the dolly-zoom couple (push
        // in → widen, pull out → narrow, keeping the subject sized — the vertigo).
        let fov_base = if s.cam_frame[1] > 1.0 { s.cam_frame[1] } else { 45.0 };
        // ⚠️ The floor is 4°, not 10°, and it is clamped in TWO places — here and in
        // `build_uniforms` (the `perspective_rh` call). They clamp the same number, so moving
        // one alone is a silent no-op. Widened by the Console Spike's Tier 1: framing a flat
        // backdrop plane wants a long lens, and 10° was the floor a near-orthographic rig ran
        // into first. `CAM_NEAR`/`CAM_FAR` did not move with it — a 127-unit plane frames at
        // ≈408 world units at 10° and ≈1023 at 4°, both well inside 0.1..5000.
        let fov_deg = (fov_base * off.fov_mul * (1.0 + s.cam_frame[2] * (1.0 - auto_dist)))
            .clamp(4.0, 120.0);
        // Substrate (Console Spike Tier 1): a third arm on this selection, and the first one
        // tried because it is the most absolute — the whole tuple is computed at once by
        // `substrate_camera::SubstrateRig::frame_plane` (cover framing for a flat plane at a
        // named vertical FOV), so nudging any single one of the six would void the framing it
        // guarantees. It lands HERE and not downstream on purpose: TAA post-multiplies
        // `view_proj`, so a matrix injected after this point fights the jitter, not rides it.
        //
        // Rails (#187): forward flight replaces the orbit. The camera sits at
        // the drag-set (bore-clamped) X/Y offset looking straight down −Z; the
        // same eye = center + distance·dir formula produces it with yaw/pitch 0
        // and the look-at center pushed down the corridor, so build_uniforms
        // (and the axes/decoration eye) need no rails-specific path. Auto-orbit
        // and scroll-zoom don't apply while riding.
        let substrate = self.substrate_rig;
        // organon#217 T3 — the held camera for a live glyph ring: a second absolute arm
        // on the same selection, below the substrate rig (the Console owns its backdrop
        // outright) and above rails and the orbit. `glyph_camera_rig` is pure: `None`
        // unless the ring is live AND `glyph_cam[0]` (hold) is set, so a session with no
        // ring — or a preset that has not asked — never enters this arm. The rig frames
        // the tiles' bounds at this frame's FOV and aspect, so the view-proj is identical
        // frame to frame and T5's accumulation is never restarted by `pt_moved`.
        let glyph_rig = glyph_camera_rig(self.glyph_pt.live, &s.glyph_cam, &self.glyph_bounds, render_size, fov_deg);
        let (cam_center, yaw, pitch, distance, cam_roll, fov_deg) = if let Some(rig) = substrate {
            rig
        } else if let Some(rig) = glyph_rig {
            rig
        } else if self.rails_ride {
            let max = self.rails_bore * 0.8;
            let off = Vec3::new(
                self.rail_off.0.clamp(-max, max),
                self.rail_off.1.clamp(-max, max),
                0.0,
            );
            (
                off + Vec3::new(0.0, 0.0, -RAILS_LOOK_AHEAD),
                0.0,
                0.0,
                RAILS_LOOK_AHEAD,
                0.0,
                45.0,
            )
        } else {
            // Truck (#307 Tier 2): slide the whole frame laterally in the camera's
            // right/up plane (scaled by the orbit radius so it reads at any distance).
            let dir = Vec3::new(
                pitch.cos() * yaw.sin(),
                pitch.sin(),
                pitch.cos() * yaw.cos(),
            );
            let right = dir.cross(Vec3::Y).normalize_or_zero();
            let up = right.cross(dir).normalize_or_zero();
            let lateral = (right * off.lat_x + up * off.lat_y) * distance * 0.5;
            (self.cam_center + lateral, yaw, pitch, distance, cam_roll, fov_deg)
        };

        // Capture decoration (#135 P5): rebuild the axes surface (tubes + cones) + box
        // back-wall lines from the params each frame (cheap). The 'X' key (`decor_on`) is a
        // master show/hide on top of the editor's per-element toggles. `eye` (same formula
        // as build_uniforms) picks which 3 box walls face away from the camera.
        let eye = cam_center
            + distance
                * Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos());
        let axes_cfg = axes::AxesConfig {
            axes_on: s.axes[0] != 0.0 && self.decor_on,
            len: s.axes[1],
            thick: s.axes[12],
            ticks: s.axes[2] != 0.0,
            axis_alpha: s.axes[4],
            box_on: s.axes[5] != 0.0 && self.decor_on,
            extent: s.axes[6],
            subdiv: s.axes[7] as u32,
            box_color: [s.axes[8], s.axes[9], s.axes[10], s.axes[11]],
            eye: eye.to_array(),
        };
        self.axes_solids = axes::build_axis_solids(&axes_cfg);
        self.box_lines = axes::build_box_lines(&axes_cfg);
        let axes_labels_on = axes_cfg.axes_on && s.axes[3] != 0.0;
        let axes_len = s.axes[1];

        // Field Chamber (#346): build the analyzer-panel geometry from the panel look
        // (Shared.chamber) + the published scope frame (Shared.scopewave) + the
        // calibrated RTA (Shared.audiospectrum). The box extent + camera `eye` (same as
        // above) pick which back walls the panels hang on. Off / no-signal → empty.
        let chamber_cfg = chamber::ChamberConfig {
            on: s.chamber[0] != 0.0 && self.decor_on,
            style: s.chamber[1] as u32,
            rear_on: s.chamber[2] != 0.0,
            right_on: s.chamber[3] != 0.0,
            extent: s.axes[6],
            eye: eye.to_array(),
            opacity: s.chamber[4],
            fill: s.chamber[5],
            scope_amp: s.chamber[6],
            // Spectrum dB window: use the SAME fixed range as the main Audio-tab RTA
            // display (lib.rs `audio_rta`, −72..0 dBFS) instead of chamber-specific dials —
            // one analyzer, one calibrated frame. (The old `chamber[7]`/`[14]` dials are no
            // longer read.)
            db_floor: -72.0,
            db_top: 0.0,
            thickness: s.chamber[12],
            emissive: s.chamber[13],
            wall_relative: s.chamber[11] != 0.0,
            frame_color: [s.axes[8], s.axes[9], s.axes[10], s.axes[11].max(0.35)],
        };
        let decor = &mut self.chamber;
        decor.surfs.clear();
        decor.lines.clear();
        decor.beads.clear();
        if chamber_cfg.on {
            let (rear_panel, right_panel) = chamber::select_walls(&chamber_cfg);
            decor.lines = chamber::build_frames(&chamber_cfg, &rear_panel, &right_panel);
            let scope_n = s.scopewave[0] as usize;
            let band_n = s.audiometer[11] as usize;
            if chamber_cfg.style == 1 {
                // Tier 2: rounded-line impostors (capsules) shaded by the IBL material.
                let mut beads = chamber::build_scope_beads(&chamber_cfg, &rear_panel, &s.scopewave[4..], scope_n);
                beads.extend(chamber::build_spectrum_beads(&chamber_cfg, &right_panel, &s.audiospectrum, band_n));
                decor.beads = beads;
            } else {
                // Tier 1: flat 2-D composite.
                let mut surfs = chamber::build_scope(&chamber_cfg, &rear_panel, &s.scopewave[4..], scope_n);
                surfs.extend(chamber::build_spectrum(&chamber_cfg, &right_panel, &s.audiospectrum, band_n));
                decor.surfs = surfs;
            }
            // Camera billboard basis (for the impostors) + material/opacity for the pass.
            let fwd = (cam_center - eye).normalize_or_zero();
            let cr = fwd.cross(Vec3::Y).normalize_or_zero();
            let cu = cr.cross(fwd);
            decor.cam_right = cr.to_array();
            decor.cam_up = cu.to_array();
            // material: [mat_type, metallic, roughness, ior]
            decor.material = [s.chamber[8], s.chamber[9], s.chamber[10], s.pbr[7].max(1.0)];
            decor.opacity = s.chamber[4];
        }

        // Terrain time-of-day: advance the day clock by `day_speed`; when running,
        // the sun rises/sets (elevation oscillates) instead of sitting at the manual
        // angle. Drives both the terrain sun and (optionally) the generator light.
        self.terrain_day += dt * s.terrain[19] as f64;
        let sun_elev_eff = if s.terrain[19] > 0.0 {
            // Full day–night arc: a high noon, then the sun plunges WELL below the
            // horizon at midnight so it gets properly dark (a sine dwells near its
            // trough → a long, deep night). A slight downward bias makes night a
            // touch longer than day. Was clamped at −8° (perpetual twilight).
            (80.0 * self.terrain_day.sin() as f32 - 8.0).clamp(-90.0, 90.0)
        } else {
            s.terrain[4]
        };

        // --- Environment source (#100) ------------------------------------------
        // Pick + (re)build the IBL/skybox env. Priority: a loaded .hdr (an explicit
        // user action — sidecar path, bumped by the plugin or our 'O' key) > the
        // physical atmosphere (the DEFAULT environment, on out of the box) >
        // procedural sky. The bake is expensive, so it runs only when the chosen
        // source's inputs change (an EnvReq value change).
        let gens = (s.hdr_gen, self.hdr.local_hdr_gen);
        if gens != self.hdr.last_hdr_gens {
            self.hdr.last_hdr_gens = gens;
            self.hdr.hdr_path = std::fs::read_to_string(ipc::hdr_sidecar_path())
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
        }

        // #472 Tier 2: procedural material bake takes precedence over the PNG set.
        // While the layer's procedural toggle (material_layer[16]) is on, (re)bake
        // the noise into its routed channel each frame (the baker no-ops unless the
        // layer params changed). On the falling edge, restore the Tier-1 PNG/neutral
        // set so the loaded folder (if any) comes back.
        let proc_on = s.material_layer[16] > 0.5;
        if proc_on {
            // #472 Tier 5: animate the material by injecting a time term into the
            // baked layers' offset/rotation. Throttle the re-bake to ~30 Hz so a live
            // material doesn't re-dispatch the whole bake every frame.
            let anim_on = s.material_live[0] > 0.5;
            let do_bake = if anim_on {
                if self.wall_time - self.last_anim_bake >= 1.0 / 30.0 {
                    self.last_anim_bake = self.wall_time;
                    true
                } else {
                    false
                }
            } else {
                true // static: the baker no-ops unless the params changed
            };
            if do_bake {
                let (l1, l2) = if anim_on {
                    let t = self.wall_time as f32;
                    (
                        animate_layer(s.material_layer, &s.material_live, t),
                        animate_layer(s.material_layer2, &s.material_live, t),
                    )
                } else {
                    (s.material_layer, s.material_layer2)
                };
                gfx.renderer.bake_material(
                    &gfx.device,
                    &gfx.queue,
                    &l1,
                    &s.material_grad,
                    &l2,
                    &s.material_grad2,
                    &s.material_derive,
                );
            }
            self.last_material_procedural = true;
        } else {
            // #472 Tier 1: (re)load the material PNG texture set when the plugin's
            // "Load Material…" button bumps `material_gen` (the hdr_gen pattern), OR
            // when procedural just turned off (restore the loaded folder / neutral).
            if s.material_gen != self.last_material_gen || self.last_material_procedural {
                self.last_material_gen = s.material_gen;
                self.last_material_procedural = false;
                let dir = std::fs::read_to_string(ipc::material_sidecar_path())
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                gfx.renderer.load_material(&gfx.device, &gfx.queue, dir.as_deref());
            }
        }
        let hdr_path = self.hdr.hdr_path.clone();
        let env_req = if hdr_path.is_some() {
            EnvReq::Hdr(s.hdr_gen, self.hdr.local_hdr_gen)
        } else if s.atmosphere[0] != 0.0 {
            let sun = dir_from_angles(sun_elev_eff, s.terrain[5]);
            // Quantize params + sun direction so a running day cycle re-bakes only
            // every ~degree of sun motion (sun.* × 64 ≈ ~0.9° steps).
            let qi = |v: f32, scale: f32| (v * scale).round() as i64;
            let keys = [
                qi(s.atmosphere[1], 100.0), // turbidity
                qi(s.atmosphere[2], 100.0), // mie_g
                qi(s.atmosphere[3], 10.0),  // sun intensity
                qi(s.atmosphere[4], 100.0), // ground albedo
                qi(s.atmosphere[5], 100.0), // exposure
                qi(s.atmosphere[7], 100.0), // rayleigh
                qi(sun.x, 64.0),
                qi(sun.y, 64.0),
                qi(sun.z, 64.0),
            ];
            let mut sig: u64 = 0xcbf2_9ce4_8422_2325;
            for k in keys {
                sig ^= k as u64;
                sig = sig.wrapping_mul(0x0000_0100_0000_01b3);
            }
            EnvReq::Atmosphere(sig)
        } else {
            EnvReq::Procedural
        };
        if self.last_env_req.as_ref() != Some(&env_req) {
            self.last_env_req = Some(env_req.clone());
            match env_req {
                EnvReq::Atmosphere(_) => {
                    let sun = dir_from_angles(sun_elev_eff, s.terrain[5]);
                    let ap = render::AtmosphereParams {
                        sun_dir: [sun.x, sun.y, sun.z],
                        sun_intensity: s.atmosphere[3],
                        turbidity: s.atmosphere[1],
                        mie_g: s.atmosphere[2],
                        ground_albedo: s.atmosphere[4],
                        rayleigh: s.atmosphere[7],
                        exposure: s.atmosphere[5],
                    };
                    gfx.renderer.load_atmosphere(&gfx.device, &gfx.queue, ap);
                }
                EnvReq::Hdr(..) => {
                    gfx.renderer.load_environment(
                        &gfx.device,
                        &gfx.queue,
                        hdr_path.as_deref().map(Path::new),
                    );
                }
                EnvReq::Procedural => {
                    gfx.renderer.load_environment(&gfx.device, &gfx.queue, None);
                }
            }
        }

        let (mut uniforms, sky_uniforms, mut post_params, ssao, mut ssr, mut ssgi) =
            build_uniforms(&s, cam_center, yaw, pitch, distance, render_size, breath_scale, sun_elev_eff, self.beat_pos as f32, cam_roll, fov_deg, gfx.renderer.material_present_mask());
        // Patch the ripple's live phase + field extent (build_uniforms has neither).
        uniforms.ripple[1] = self.ripple_phase as f32;
        // organon#217 T3: while a glyph ring is drawing, the tiles ARE the generator's
        // instanced-cube draw (they replaced its instances), so `Uniforms.shape` carries
        // the glyph look's own bevel and face crown instead of the Generator bucket's
        // `bevel`. `glyph_shape` is pure and pinned; with no ring it returns the frame's
        // own lanes untouched, byte-identical to before.
        uniforms.shape = glyph_shape(uniforms.shape, self.glyph_pt.live, &s.glyph);
        // Demo point light (#288 Tier 3): drive the shader's placeable light from the
        // brightest scene emitter, so a light-stage / ceiling emitter actually
        // illuminates (on top of blooming as emissive geometry). Off for every other
        // generator (demo_lights is empty), so the term stays inert (byte-identical).
        if let Some(l) = self
            .demo_lights
            .iter()
            .max_by(|a, b| a.intensity.partial_cmp(&b.intensity).unwrap_or(std::cmp::Ordering::Equal))
        {
            uniforms.demo_light_pos = [l.pos.x, l.pos.y, l.pos.z, l.intensity];
            uniforms.demo_light_col = [l.color.x, l.color.y, l.color.z, l.radius.max(1e-3)];
        }
        // Stochastic transparency (#152 Tier 2): the spare uniform slots sss.w / irid.w
        // carry the enable flag + an animated seed (per-frame, so TAA resolves the
        // dither). Read only by cube.wgsl's Glass branch; 0 = classic alpha blend.
        let stochastic_on = s.temporal[6] != 0.0;
        let frame_seed = (self.wall_time * 60.0) as u32; // ~per-frame counter
        uniforms.sss[3] = if stochastic_on { 1.0 } else { 0.0 };
        uniforms.irid[3] = (frame_seed % 64) as f32;
        // SSGI frame seed (per-frame jitter for the ray dirs; TAA denoises).
        ssgi.extra[1] = (frame_seed % 256) as f32;
        // SSR frame seed (#174 T3: stochastic-roughness ray jitter; TAA denoises).
        ssr.ssr[3] = (frame_seed % 256) as f32;
        let span = bounds.max - bounds.min;
        uniforms.ripple_ctr[3] = if span.is_finite() {
            (span.length() * 0.5).max(1e-3)
        } else {
            1.0
        };
        // #163 Tier 2: parallax reflection box. When the source is Parallax, turn the
        // live field AABB (scaled by the box params) into the world-space box the cube
        // shader box-projects the env reflection against, and flag it on (w = 1). source
        // 0 (or a degenerate/empty field) leaves refl_box_* at 0 → today's reflection.
        if s.refl_probe[0] > 0.5 && span.is_finite() {
            let center = (bounds.min + bounds.max) * 0.5;
            let half = (bounds.max - bounds.min) * 0.5;
            let sx = s.refl_probe[1].max(1e-3);
            let sy = s.refl_probe[2].max(1e-3);
            let ext = Vec3::new(
                (half.x * sx).max(1e-3),
                (half.y * sy).max(1e-3),
                (half.z * sx).max(1e-3),
            );
            let bmin = center - ext;
            let bmax = center + ext;
            uniforms.refl_box_min = [bmin.x, bmin.y, bmin.z, 1.0]; // w = source on
            uniforms.refl_box_max = [bmax.x, bmax.y, bmax.z, s.refl_probe[3].clamp(0.0, 1.0)];
        }
        let ssao_on = s.ssao[0] != 0.0;
        // #174 T3: specular occlusion rides SSAO. The flag travels in glassx.w — a
        // uniform spare cube.wgsl never read (the old "spectral_samples") — and the
        // renderer binds the blurred AO (or a white no-op dummy) into group 3.
        uniforms.glassx[3] = if ssao_on { 1.0 } else { 0.0 };
        // Jewel Box (#80): SSR + bounced-GI gating + the CPU probe grid. GI probes
        // are computed from the live node positions (instance translations) + tints
        // only when GI is on, so the off path costs nothing.
        let ssr_on = s.ssr[0] != 0.0;
        let ssgi_on = s.ssgi[0] != 0.0;
        // Cast shadows (#152 Tier 3): a key-light ortho depth map. The light follows
        // the same key elevation/azimuth as the lighting (or the terrain sun when it
        // drives the scene), covering the scene bounds.
        let shadow_on = s.shadow[0] != 0.0;
        // #182 T4: the fluid transmittance / caustic passes project from the
        // same key-light matrix, so compute it when they want it too.
        let fluidlight_wants = s.fluidgi[1] > 0.0 || s.caustic[0] > 0.0;
        let shadow_light_vp = if shadow_on || fluidlight_wants {
            let key_dir = if s.terrain[0] != 0.0 && s.terrain[26] > 0.5 {
                dir_from_angles(sun_elev_eff, s.terrain[5])
            } else {
                dir_from_angles(s.lighting[3], s.lighting[4])
            };
            shadow_light_matrix(key_dir, bounds.min, bounds.max)
        } else {
            Mat4::IDENTITY.to_cols_array_2d()
        };
        // #256 T2 — cache GI supersedes the DDGI probe grid: when the Tier-0 cache is
        // live and cache-GI is on, the 6³ SH probe volume is filled from the cache
        // instead of the discrete node integration (below), with its own strength.
        // Forces the probe path on + provides the intensity so it's a standalone
        // toggle. Off → gi_on/gi_intensity are exactly s.gi[0]/s.gi[1] (byte-identical).
        let nrc_gi_on = s.nrc[0] > 0.5 && s.nrc3[0] > 0.5;
        let gi_on = s.gi[0] != 0.0 || nrc_gi_on;
        let gi_intensity = if nrc_gi_on { s.nrc3[1] } else { s.gi[1] };
        let gi_falloff = s.gi[2];
        // #182: the GI/VXGI light volume covers everything that can RECEIVE the
        // bounce, not just the generator — union the scene bounds with the
        // liquid tank when it's on. Otherwise the volumes end at the
        // generator's AABB and the water shows a hard cubic cutoff where the
        // bounce dies (the probes freeze at their edge value; the VXGI sample
        // hard-zeroes). A larger volume also means larger splat/probe cells, so
        // the light physically carries further into the pool.
        let (light_min, light_max) = {
            let mut lo = bounds.min;
            let mut hi = bounds.max;
            if s.liquid[0] != 0.0 {
                let half = s.liquid[6].max(1.0);
                let c = self.cam_center + Vec3::Y * s.liquid2[0];
                lo = lo.min(c - Vec3::splat(half));
                hi = hi.max(c + Vec3::splat(half));
            }
            (lo, hi)
        };
        // Contiguous (welded) Swept Tubes clears `instances`; fall back to the welded
        // per-segment node anchors (filled when `need_weld_nodes`, which now includes
        // `gi_on`) so the bounced-GI probe volume still fills in welded mode.
        let (gi_inst, gi_tint): (&[Mat4], &[Vec4]) =
            if !self.geom.node_insts_weld.is_empty() {
                (&self.geom.node_insts_weld, &self.geom.node_tints_weld)
            } else {
                (&self.geom.instances, &self.geom.tints)
            };
        let mut gi_probes: Vec<Vec4> = if nrc_gi_on {
            // #256 T2 — supersede the discrete grid with the continuous cache. The fill
            // (`math::compute_gi_probes_from_cache`) runs AFTER the NRC training block
            // below, so the probes query THIS frame's freshly created + trained cache
            // (no first-frame `None`, no one-frame lag) with the training-bounds encode
            // (`bounds`, not the larger GI volume). Placeholder here.
            Vec::new()
        } else if gi_on && !gi_inst.is_empty() {
            // Pre-stride to the probe sample cap BEFORE collecting (#174 T2): the
            // old code collected EVERY node's position (a 12 MB allocation per
            // frame at 1M nodes) only for compute_gi_probes to stride it back down
            // to ≤ GI_MAX_SAMPLES internally. Same stride arithmetic → the exact
            // same sample set.
            let stride = (gi_inst.len() / math::GI_MAX_SAMPLES).max(1);
            let positions: Vec<Vec3> =
                gi_inst.iter().step_by(stride).map(|m| m.w_axis.truncate()).collect();
            let tints: Vec<Vec4> = gi_tint.iter().step_by(stride).copied().collect();
            let mut probes =
                math::compute_gi_probes(&positions, &tints, light_min, light_max, gi_falloff);
            // A node's tint is its ALBEDO, not its emission: scale the injected
            // bounce by glow + a key-light estimate (same convention as the VXGI /
            // many-lights injection in render.rs) so an unlit, non-emissive field
            // doesn't bleed colour at full strength. SH coefficients are linear in
            // the input colour, so one scalar on the packed Vec4s is exact.
            let radiance_scale =
                uniforms.mat[2].max(0.0) + 0.3 * uniforms.key_light[3].max(0.0);
            for p in probes.iter_mut() {
                *p *= radiance_scale;
            }
            probes
        } else {
            Vec::new()
        };
        // HDR output (macOS EDR): feed the measured headroom to the composite
        // tonemap so highlights roll off toward it instead of clamping to white.
        // #430: while recording HDR, drive the composite with the *recording* mastering
        // headroom instead of the display's — so the file preserves the highlight range
        // the renderer generates, even beyond what the attached panel can show (the
        // on-screen preview may clip brighter during a take; the file gets the full range).
        // #541 S2 T3: both of these are properties of the *display* the frame is
        // presented on, so `frame_hdr_max` / `frame_gamut` force an offscreen
        // target back to plain SDR — there is no CAMetalLayer behind it.
        post_params.hdr_max = frame_hdr_max(
            &out,
            self.hdr.hdr_enabled,
            self.hdr.hdr_max,
            self.record.recorder.is_some().then(recorder::record_headroom),
        );
        // SDR output-dither clock (#174 T3).
        post_params.time = self.wall_time as f32;
        // Wide-gamut output (#119): the EDR surface is tagged Rec.2020 only when HDR
        // is on AND wide gamut is enabled — gate the composite's gamut expansion to
        // match, so it never runs against a Rec.709 surface.
        // `target.wide_gamut`, not `self.hdr.hdr_wide`: the world's flag is what the user *asked*
        // for, the target's is whether the host actually tagged its surface Rec.2020. Only the
        // host knows, and expanding into a gamut the surface was never tagged for is exactly the
        // primaries mismatch #554 T4 could not test for.
        post_params.gamut = frame_gamut(&out, self.hdr.hdr_enabled, target.wide_gamut);

        // Terrain backdrop: advance the fly clock by the fly speed (so changing
        // speed never jumps the camera), then build its uniforms. The synthetic
        // fly-camera rides the landscape; ray directions come from the orbit camera.
        let terrain_on = s.terrain[0] != 0.0;
        self.terrain_time += dt * s.terrain[7] as f64;
        let terrain_u =
            build_terrain_uniforms(&s, &sky_uniforms, &self.terrain_noise, self.terrain_time, sun_elev_eff);

        // Starfield: wheel the sky by the sidereal-rotation speed (so changing speed
        // never jumps the sky), then build its uniforms. The night factor fades the
        // stars in as the day-cycle sun sets; the sun disc rides the same sun.
        let stars_on = s.stars[0] != 0.0;
        let star_sun = s.stars[9] != 0.0;
        self.sky_time += dt * s.stars[6] as f64;
        self.wall_time += dt;
        let star_u =
            build_star_uniforms(&s, &sky_uniforms, self.sky_time, self.wall_time, sun_elev_eff, render_size);

        // #102B FFT ocean: synthesise + upload the Tessendorf wave tile each frame
        // (a no-op when the ocean is off). Animated on the wall clock so the swell
        // keeps rolling regardless of the fly speed.
        let ocean_on = s.ocean[0] != 0.0;
        let ocean_params = render::OceanParams {
            wind_speed: s.ocean[2],
            wind_dir_deg: s.ocean[3],
            amplitude: s.ocean[4],
            choppiness: s.ocean[5],
            tile_size: s.ocean[6].max(1.0),
        };
        gfx.renderer.update_ocean(&gfx.queue, ocean_on, ocean_params, self.wall_time as f32);

        // --- Particle Aura (#81): build the velocity field + respawn anchors -----
        // The motes ride the active generator's flow through one abstraction: the
        // analytic field where the generator exposes one (curl-noise / attractor /
        // Maxwell), the node-motion splat grid otherwise. Off (or a node-less
        // generator like Mandelbulb) → inert; the renderer dispatches nothing.
        let aura = &mut self.particle_aura;
        aura.seed = aura.seed.wrapping_add(1);
        // Fluid Ink (#182 Tier 1): the dye rides the Aura-Fluid solver, so the
        // whole stir-field block below also runs (and the solver steps) whenever
        // the ink is on — even with the Particle Aura off. The MLS-MPM liquid
        // (#182 T3a) also wants the per-node bookkeeping when its colliders are
        // on (positions + velocities for the occupancy grid).
        // #187 pure ride (#203 review): with the generator off, the corridor
        // IS the world geometry (rail space == world space), so the
        // node-driven systems (particle aura, fluid ink dye, liquid colliders)
        // read the scenery instances. Composite-mode scenery is view-locked
        // (eye space, not world) and stays excluded — only the generator's
        // instances feed them there.
        // Contiguous Swept Tubes clears `instances` (the raster draws the welded mesh),
        // so fall back to the welded per-segment node anchors (`node_insts_weld`, filled
        // above when a node system is live) — else the aura/ink/liquid colliders would
        // die whenever Contiguous is on. Rails-ride scenery keeps its existing priority.
        let node_insts: &[Mat4] = if self.rails_ride && self.geom.instances.is_empty() {
            &self.scenery_instances
        } else if !self.geom.node_insts_weld.is_empty() {
            &self.geom.node_insts_weld
        } else {
            &self.geom.instances
        };
        let node_tints: &[Vec4] = if self.rails_ride && self.geom.instances.is_empty() {
            &self.scenery_tints
        } else if self.geom.instances.is_empty() && !self.geom.node_tints_weld.is_empty() {
            &self.geom.node_tints_weld
        } else {
            &self.geom.tints
        };
        let ink_on = s.fluidvis[0] != 0.0 && !node_insts.is_empty();
        let liq_on = s.liquid[0] != 0.0;
        let liq_nodes = liq_on && s.liquid[8] != 0.0 && !node_insts.is_empty();
        let particle_frame = {
            let tier = s.particles[0] as u32;
            let fluid = tier == 2;
            // The solver also runs for the ink; the motes only RIDE it at Fluid.
            let solver = fluid || ink_on;
            let enabled = tier >= 1 && !node_insts.is_empty();
            if enabled || ink_on || liq_nodes {
                let gen = GeneratorMode::from_u32(s.generator);
                let count = ((s.particles[1].max(0.0) * 1000.0) as u32).max(1);
                // The fluid solver assumes a cubic grid; cap its resolution lower
                // (a full NS solve is far heavier than plain advection). #182 T2:
                // an explicit override lifts the solver cap to 128³ (an honest
                // perf dial — the buffers + every pass scale with it).
                let fl2_res = s.fluid2[6] as u32;
                let res = if solver {
                    if fl2_res >= 8 {
                        fl2_res.min(128)
                    } else {
                        (s.particles[2] as u32).clamp(8, 64)
                    }
                } else {
                    (s.particles[2] as u32).clamp(8, 96)
                };
                let resv = [res, res, res];

                // Node positions (= instance translations), stride-sampled to a cap:
                // both the respawn anchors and the splat source.
                aura.node_samples.clear();
                let stride = (node_insts.len() / MAX_NODE_SAMPLES).max(1);
                for m in node_insts.iter().step_by(stride) {
                    let p = m.w_axis.truncate();
                    aura.node_samples.push(Vec4::new(p.x, p.y, p.z, 0.0));
                }

                // Field AABB = the scene bounds, padded so the cloud has room to
                // drift past the structure. The Fluid tier needs a CUBE (uniform
                // cell size for the finite-difference operators), so square it up.
                let span = bounds.max - bounds.min;
                let pad = (span * 0.25).max(Vec3::splat(1.0));
                let mut gmin = bounds.min - pad;
                let mut gmax = bounds.max + pad;
                if solver {
                    let center = (gmin + gmax) * 0.5;
                    let half = ((gmax - gmin) * 0.5).max_element().max(0.5);
                    gmin = center - Vec3::splat(half);
                    gmax = center + Vec3::splat(half);
                }
                if aura.vel_grid.res != resv {
                    aura.vel_grid = math::VelGrid::new(resv, gmin, gmax);
                } else {
                    aura.vel_grid.min = gmin;
                    aura.vel_grid.max = gmax;
                    aura.vel_grid.clear();
                }

                // Per-node finite-difference velocities (only when the node set
                // matches last frame, so a param change that resizes the field
                // skips one frame rather than smearing). Computed for EVERY
                // generator mode: the node-splat arm below stirs with them, and
                // the solid boundaries (#182 T2) need them even in the analytic
                // arms — a wall must move with its NODE, not with the analytic
                // field value that happens to fill `source.xyz` there.
                let cur: Vec<Vec3> = aura.node_samples.iter().map(|p| p.truncate()).collect();
                let have_vels = aura.prev_node_pos.len() == cur.len() && !cur.is_empty();
                if have_vels {
                    math::node_velocities(
                        &aura.prev_node_pos,
                        &cur,
                        dt as f32,
                        &mut aura.node_vels,
                    );
                } else {
                    aura.node_vels.clear();
                }

                match gen {
                    GeneratorMode::CurlNoise => {
                        let c = &s.cn;
                        aura.vel_grid.fill_analytic(&math::AnalyticField::CurlNoise {
                            scale: c[3],
                            t: self.gen_phase as f32 * c[6],
                            bound: c[7],
                            seed: c[1] as u32,
                        });
                    }
                    GeneratorMode::Attractor => {
                        let a = &s.attr;
                        let field = a[0] as u32;
                        aura.vel_grid.fill_analytic(&math::AnalyticField::Attractor {
                            field,
                            scale: math::attractor_scale(field) * a[7],
                        });
                    }
                    GeneratorMode::MaxwellField => {
                        let m = &s.maxwell;
                        // #248 Tier 3: the pitch-scaled field clock + the stereo lean feed
                        // both source modes (clock hard-synced to gen_phase when the drive
                        // is off → byte-identical pre-#248).
                        let fphase = self.maxdip_phase as f32;
                        let lean = audio_stereo_lean(&s);
                        // #feat/maxwell-eb-blend: the aura traces an INDEPENDENT E↔B blend
                        // ∈ [0,1] (0 = pure E, 1 = pure B, 0.5 = equal mix) — separate from
                        // the generator's arrows, so the motes can flow along a different
                        // field/mix than the lattice draws. It drives BOTH the aura's motion
                        // AND its glow energy density together (threaded into the field's
                        // `blend`, which velocity() + energy() both read).
                        let aura_blend = s.maxenergy[7];
                        // #248 Tier 2: use the band-multipole field for energization
                        // only when it actually has elements — mirror the arrows'
                        // `band_mode` gate. Otherwise (multipole off, or on-but-silent
                        // with floor 0 → no bands) fall through to the Tier-1 point /
                        // antenna path so motes/dye/liquid track the SAME field the
                        // arrows draw, instead of reading a dark grid (Bugbot:
                        // empty-band mismatch).
                        let band_elems = if audio_multipole_on(&s) {
                            audio_band_elems(&s)
                        } else {
                            Vec::new()
                        };
                        let field = if !band_elems.is_empty() {
                            // The band stack replaces the point/antenna field — the
                            // energy cloud's SHAPE encodes the spectrum (bass fattens the
                            // low-order lobe, cymbals sparkle the high-order structure).
                            math::AnalyticField::MaxwellBands {
                                elems: band_elems,
                                blend: aura_blend,
                                near: m[7],
                                r_min: m[10],
                                phase: fphase,
                            }
                        } else {
                            let dipoles = m[3] > 0.5;
                            let mut sources = math::maxwell_sources(
                                (m[2] as usize).max(1),
                                m[4],
                                dipoles,
                                m[5],
                                m[6],
                                self.gen_phase as f32,
                            );
                            if lean != 0.0 {
                                for src in &mut sources {
                                    src.pos.x += lean;
                                }
                            }
                            // #247 Tier 2: the finite antenna (standing-wave current on a
                            // rod) replaces the point field for energization when on
                            // (maxenergy[5]) — 64 quadrature segments; the near-field bound
                            // charge peaks at the tips → the bright-ends/dim-centre demo.
                            let antenna_segs = if s.maxenergy[5] != 0.0 { 64 } else { 0 };
                            math::AnalyticField::Maxwell {
                                sources,
                                dipoles,
                                blend: aura_blend,
                                k: m[8],
                                near: m[7],
                                r_min: m[10],
                                phase: fphase,
                                antenna_len: s.maxenergy[4],
                                antenna_segs,
                                // #248 Tier 1: the music's loudness envelope drives the
                                // source amplitude → the energy cloud (mote glow + Tier-3
                                // dye, both read this grid) breathes with the track.
                                drive: audio_dipole_drive(&s),
                                // #248 Tier 3: lean the finite-antenna rod with the mix, so
                                // it agrees with the leaned sources + the shell centre.
                                offset: Vec3::new(lean, 0.0, 0.0),
                            }
                        };
                        // #248 field-force drive: stir the medium by the field instead of
                        // sliding along field lines at constant speed. `rotational` is
                        // `solver` — TRUE whenever the grid feeds an incompressible solve
                        // (Fluid aura tier OR Fluid Ink): there it must be the solenoidal
                        // circulation `stir` (a conservative E would be projected away). It's
                        // FALSE only for pure direct mote advection (Lite aura, no solver),
                        // where the E `force` pushes the motes like charges. `energy_contrast`
                        // (mxforce[2]) is applied at display time (mote shader / dye), so it
                        // works in both tiers.
                        if s.mxforce[0] != 0.0 {
                            // `pulse_env` is the PLL beat envelope (or the live audio bass
                            // when Pulse-source = audio) — the audio-reactive driver.
                            let pulse_on = s.pulse != 0;
                            let beat_env = if pulse_on { pulse_env } else { 0.0 };
                            let beat_force = s.mxforce2[1];
                            let pump_amount = s.mxforce2[0];
                            // The manual slow reversal (`stir rate` Hz) — the baseline swirl
                            // whenever the beat isn't driving (Pulse off, or beat force 0),
                            // so the field never freezes for want of a beat (Bugbot).
                            let osc_manual = (std::f64::consts::TAU
                                * s.mxforce[3] as f64
                                * self.wall_time)
                                .cos() as f32;

                            // --- Engine A: TURBINE + independent pump (beat mode −1). Each
                            // beat kicks angular momentum ONE direction; it coasts down at
                            // `swirl_slowdown`. No beat driving (Pulse off / force 0) → the
                            // manual reversal.
                            self.swirl_spin += beat_env as f64 * beat_force as f64 * dt;
                            self.swirl_spin *= (1.0 - s.mxforce2[3] as f64 * dt).clamp(0.0, 1.0);
                            self.swirl_spin = self.swirl_spin.clamp(0.0, 8.0);
                            let osc_a = if beat_force > 0.0 && pulse_on {
                                self.swirl_spin as f32
                            } else {
                                osc_manual
                            };
                            let pump_a = pump_amount * beat_env;

                            // --- Engine B: COUPLED E↔B DYNAMO (beat mode +1). The beat kicks
                            // a struck cavity — the axial (E) mode induces the swirl (B),
                            // which feeds back into the pump: energy RINGS between them and
                            // decays. Pump rides E (axial in/out), swirl rides B (reverses).
                            let (e, b) = math::em_cavity_step(
                                self.em_e,
                                self.em_b,
                                (std::f64::consts::TAU * s.mxforce3[1] as f64) as f32, // ring ω
                                s.mxforce2[3],                                          // ring-down
                                beat_env * beat_force,                                  // beat kick → E
                                dt as f32,
                            );
                            self.em_e = e.clamp(-8.0, 8.0);
                            self.em_b = b.clamp(-8.0, 8.0);
                            // With no beat striking the cavity (Pulse off) it rings down to
                            // nothing — fall back to the manual reversal so the swirl holds.
                            let osc_b = if pulse_on { self.em_b } else { osc_manual };
                            let pump_b = pump_amount * self.em_e;

                            // Crossfade the two engines: mode_mix −1 → all A, +1 → all B,
                            // 0 → an even blend. Both run every frame, so it's smooth.
                            let wb = (s.mxforce3[0].clamp(-1.0, 1.0) + 1.0) * 0.5;
                            let wa = 1.0 - wb;
                            // E↔B LOCK: with Tempo Sync on (maxwell[22]), the E oscillation is
                            // slow/musical enough for the fluid to follow, so reverse the B
                            // swirl on the SAME clock that pumps E (`maxdip_phase`, now the
                            // beat-locked phase) instead of the arbitrary wall-time `stir rate`.
                            // `cos(ωt)` is E's own temporal reversal, so the swirl (B) and the
                            // field lines (E) flip together — one coupled oscillation at one
                            // frequency. The **E↔B phase** dial (`mx_eb[0]`, degrees) offsets
                            // the swirl relative to the E clock: 0° = far-field (E∥B in phase —
                            // the radiation B the code computes); 90° = near-field induction
                            // (quadrature — the swirl peaks at E's zero-crossing, as ∂B/∂t ∝ ∇×E
                            // demands near the source, `cos(ωt−90°) = sin(ωt)`). The turbine/
                            // dynamo/manual engines remain the Tempo-Sync-OFF look — presets unchanged.
                            let osc = if s.maxwell[22] > 0.5 {
                                let phi = s.mx_eb[0].to_radians();
                                (self.maxdip_phase as f32 - phi).cos()
                            } else {
                                osc_a * wa + osc_b * wb
                            };
                            let pump_env = pump_a * wa + pump_b * wb;
                            // Aura E↔B blend (maxenergy[7]) now drives the force-drive
                            // too: 0 = E force, 1 = B swirl (was hard-switched by `solver`).
                            aura.vel_grid.fill_analytic_force(
                                &field, s.mxforce[1], s.maxenergy[7], osc, pump_env, s.mxforce2[2],
                            );
                        } else {
                            aura.vel_grid.fill_analytic(&field);
                        }
                        // #248 Tier 3: waveform-as-retarded-amplitude — the recent
                        // loudness history modulates the baked energy radially (now at
                        // the source, older toward the rim), so loud moments radiate
                        // outward as bright shells through the energy cloud.
                        if s.audiodip[0] != 0.0 && s.audiodip2[0] > 0.0 {
                            let span =
                                0.5 * (aura.vel_grid.max - aura.vel_grid.min).max_element();
                            aura.vel_grid.modulate_energy_radial(
                                Vec3::new(lean, 0.0, 0.0),
                                &self.rms_hist,
                                span,
                                s.audiodip2[0],
                            );
                        }
                    }
                    GeneratorMode::Acoustic => {
                        // #325 Tier 2: the acoustic Duo-Field particle channel — motes
                        // advect along the compression↔transverse blend (aura_blend 1 =
                        // the transverse flow) and glow by the acoustic energy density.
                        // Tier 3 shares the audio spine: RMS + beat pump scale the drive,
                        // spectrum → the multipole stack, stereo leans the source. Tier 4
                        // switches to the cavity eigenmode and/or the intensity-flux channel.
                        let a = &s.acoustic;
                        let a2 = &s.acoustic2;
                        let phase = self.maxdip_phase as f32;
                        let intensity = a2[6];
                        if a2[0] > 0.5 {
                            // #325 Tier 4/5: cavity standing-wave aura — tweened 3-D
                            // mode walk + per-axis audio breathe, matching the geometry
                            // arm so the motes and the shell agree.
                            let a3 = &s.acoustic3;
                            let base_modes = Vec3::new(a2[1], a2[2], a2[3]);
                            let mut modes = math::cavity_morph_modes_tween(
                                base_modes,
                                self.beat_pos,
                                a2[4],
                                a3[0],
                            );
                            modes += Vec3::new(a3[1], a3[2], a3[3]) * cavity_audio_breathe(&s);
                            let dims = Vec3::splat(a2[5].max(1.0e-3));
                            let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                            let field = math::AnalyticField::AcousticCavity {
                                modes,
                                dims,
                                blend: a[14],
                                phase,
                                drive: audio_dipole_drive(&s) * pump,
                                intensity,
                            };
                            aura.vel_grid.fill_analytic(&field);
                        } else {
                            let kind = math::AcousticKind::from_u32(a[0] as u32);
                            let pump = 1.0 + a[15] * if s.pulse != 0 { pulse_env } else { 0.0 };
                            // Same band-mode handling as the geometry arm: fall back to the
                            // static layout when every band is silent, and skip the broadband
                            // drive in genuine band mode (band weights already in each `q`).
                            let band_drives = audio_band_drives(&s);
                            let use_bands =
                                audio_multipole_on(&s) && band_drives.iter().any(|d| *d > 0.0);
                            let mut sources = if use_bands {
                                math::acoustic_band_sources(&band_drives, a[4])
                            } else {
                                math::acoustic_sources(kind, a[4])
                            };
                            let drive = if use_bands { 1.0 } else { audio_dipole_drive(&s) };
                            let lean = if s.audiodip[0] != 0.0 {
                                s.audio[7].clamp(-1.0, 1.0) * s.audiodip[6].clamp(0.0, 1.0) * a[4].max(1.0)
                            } else {
                                0.0
                            };
                            if lean != 0.0 {
                                for src in &mut sources {
                                    src.pos.x += lean;
                                }
                            }
                            let field = math::AnalyticField::Acoustic {
                                sources,
                                blend: a[14],
                                k: a[1],
                                near: a[2],
                                r_min: a[5],
                                phase,
                                drive: drive * pump,
                                intensity,
                            };
                            aura.vel_grid.fill_analytic(&field);
                        }
                    }
                    GeneratorMode::FieldEngine => {
                        // #381: a Vector field program advects the aura + glows by
                        // its energy density `|F|²`, exactly like Maxwell/Acoustic.
                        // Scalar/Complex programs have no natural flow → no aura fill
                        // (their glyph lattice carries the look). The program was
                        // already (re)compiled by the geometry pass this frame, so we
                        // read the cached `self.field_prog.field_program` directly here — a
                        // method call would re-borrow all of `self` while `gfx` is
                        // held.
                        let f = &s.field;
                        let is_vector = match organon_core::params::FieldKind::from_u32(f[0] as u32) {
                            organon_core::params::FieldKind::Vector => true,
                            organon_core::params::FieldKind::Auto => self
                                .field_prog.field_program
                                .as_ref()
                                .map(|p| p.kind() == math::FieldValKind::Vector)
                                .unwrap_or(false),
                            _ => false,
                        };
                        // Tier 3 (a live PDE sim) has no analytic flow field → skip the
                        // aura fill; the sim's glyph lattice carries the look. Gate on the
                        // SAME `PdePreset` decode the geometry pass uses, so an out-of-range
                        // preset id (nonzero but → `Off`) can't render the static field here
                        // yet still fill the aura.
                        let pde_active =
                            math::PdePreset::from_u32(s.fieldsim[0] as u32) != math::PdePreset::Off;
                        if let (true, false, Some(base)) =
                            (is_vector, pde_active, self.field_prog.field_program.clone())
                        {
                            let prog = base
                                .with_bindings(self.gen_phase as f32, f[4], f[5])
                                .with_scale(f[2].max(1.0e-3));
                            aura.vel_grid.fill_analytic(&math::AnalyticField::Field { program: prog });
                        }
                    }
                    _ => {
                        // Aura Tier 2: splat the per-node finite-difference
                        // velocities (computed above) as the stir source.
                        if have_vels {
                            for (p, v) in cur.iter().zip(aura.node_vels.iter()) {
                                aura.vel_grid.splat(*p, *v);
                            }
                            aura.vel_grid.finalize_splat();
                        }
                    }
                }
                aura.prev_node_pos = cur;
                // Bake Maxwell energy into `w` ONLY when no solver rides this buffer:
                // the NS solver reads `w` as its solid-wall occupancy (`source.w > 0.5`),
                // so energy in `w` would spawn phantom walls (#247 fix). In Fluid mode the
                // motes glow from the solver's own `cs_energy` pass, not this buffer.
                aura.vel_grid.to_vec4(&mut aura.vel_upload, !solver);

                // #182 Tier 2 — solid boundaries: mark node-occupied cells in the
                // upload's w channel, each carrying its node's velocity so the
                // whole wall shell moves WITH the node (the ball is wider than
                // the trilinear splat, and the analytic arms fill xyz with the
                // field, not node motion). Radius ≈ a node's world size in cells
                // (clamped so fine grids don't explode the stencil, coarse ones
                // still get a wall). First frame after a node-count change has no
                // velocities yet → stationary walls for that one frame.
                if solver && s.fluid2[0] != 0.0 {
                    let cell = ((gmax.x - gmin.x) / res.max(1) as f32).max(1e-4);
                    let occ_r = (0.6 / cell).clamp(1.0, 2.5);
                    for (i, p) in aura.node_samples.iter().enumerate() {
                        let v = aura.node_vels.get(i).copied().unwrap_or(Vec3::ZERO);
                        aura.vel_grid.mark_occupancy(p.truncate(), v, occ_r, &mut aura.vel_upload);
                    }
                }

                // Fluid Ink (#182 Tier 1): splat each node's colour into the dye
                // injection grid — the ink a Lorenz attractor sheds is its palette.
                // Colour source matches the metaball/VXGI node build: the live tint
                // when a palette/HSV sweep is active, else the RGB cube by position.
                if ink_on {
                    if self.fluid_grids.dye.res != resv {
                        self.fluid_grids.dye = math::VelGrid::new(resv, gmin, gmax);
                    } else {
                        self.fluid_grids.dye.min = gmin;
                        self.fluid_grids.dye.max = gmax;
                        self.fluid_grids.dye.clear();
                    }
                    let palette_active = s.surface_fx[6] != 0.0;
                    let cspan = (bounds.max - bounds.min).max(Vec3::splat(1e-3));
                    let radius = s.fluidvis[2].clamp(0.5, 4.0);
                    let stride = (node_insts.len() / MAX_DYE_NODES).max(1);
                    // #247 Tier 3: energized Maxwell → inject BRIGHT dye by the field
                    // energy density (the same `w` energy the motes glow by, sampled from
                    // the vel grid), tone-mapped like the mote glow and tinted by the
                    // ember hue. The fluid then advects + swirls the glow — energy
                    // visibly FLOWING through the field, not just marking it. Needs Fluid
                    // Ink on to render; strength 0 (maxenergy[6]) → the plain node colour.
                    let energy_dye = s.maxenergy[0] != 0.0
                        && gen == GeneratorMode::MaxwellField
                        && s.maxenergy[6] != 0.0;
                    let dye_strength = s.maxenergy[6];
                    let dye_gain = s.maxenergy[1];
                    let dye_knee = s.maxenergy[2];
                    let dye_contrast = s.mxforce[2].max(0.05); // #248 near-core contrast
                    let dye_ember = math::hsv_tint(s.maxenergy[3]).truncate();
                    // #248 Tier 2: colour by band — in multipole mode the dye's hue
                    // blends toward the energy-weighted band hue at each node (bass
                    // holds the ember, highs pull across the wheel).
                    let dye_band_elems = if energy_dye
                        && audio_multipole_on(&s)
                        && s.audiodip[5] > 0.0
                    {
                        audio_band_elems(&s)
                    } else {
                        Vec::new()
                    };
                    // #248 Tier 3: the waveform shells apply to the direct band-math
                    // path too (the grid path already carries them in `w`).
                    let dye_wave = if s.audiodip[0] != 0.0 { s.audiodip2[0] } else { 0.0 };
                    let dye_wave_span = 0.5 * (aura.vel_grid.max - aura.vel_grid.min).max_element();
                    let dye_lean = audio_stereo_lean(&s);
                    for (m, t) in
                        node_insts.iter().zip(node_tints.iter()).step_by(stride)
                    {
                        let p = m.w_axis.truncate();
                        let c = if energy_dye {
                            if dye_band_elems.is_empty() {
                                let u = aura.vel_grid.sample_energy(p).powf(dye_contrast);
                                let mapped = math::energy_tonemap(u, dye_gain, dye_knee);
                                dye_ember * (mapped * dye_strength)
                            } else {
                                let (per, mut tot) = math::maxwell_band_energies(
                                    p,
                                    &dye_band_elems,
                                    s.maxenergy[7], // aura E↔B blend → glow follows the selected field
                                    s.maxwell[7],
                                    s.maxwell[10],
                                    self.maxdip_phase as f32,
                                );
                                if dye_wave > 0.0 {
                                    let r = (p - Vec3::new(dye_lean, 0.0, 0.0)).length();
                                    let g =
                                        math::radial_wave_gain(&self.rms_hist, dye_wave_span, r);
                                    tot *= 1.0 + dye_wave.clamp(0.0, 1.0) * math::WAVE_BOOST * g;
                                }
                                let mapped =
                                    math::energy_tonemap(tot.powf(dye_contrast), dye_gain, dye_knee);
                                let band = math::hsv_tint(math::band_hue_blend(
                                    &per,
                                    s.maxenergy[3],
                                ))
                                .truncate();
                                dye_ember
                                    .lerp(band, s.audiodip[5].clamp(0.0, 1.0))
                                    * (mapped * dye_strength)
                            }
                        } else if palette_active {
                            Vec3::new(t.x, t.y, t.z)
                        } else {
                            (p - bounds.min) / cspan
                        };
                        self.fluid_grids.dye.splat_ball(p, c, radius);
                    }
                    self.fluid_grids.dye.to_vec4(&mut self.fluid_grids.dye_upload, false);
                } else {
                    self.fluid_grids.dye_upload.clear();
                }

                // Camera basis (for the billboards) + the UNSCALED world→clip the
                // motes project through (they live in true world space).
                let eye = cam_center
                    + distance
                        * Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos());
                let fwd = (cam_center - eye).normalize_or_zero();
                let cam_right = fwd.cross(Vec3::Y).normalize_or_zero();
                let cam_up = cam_right.cross(fwd);
                let view_proj = Mat4::from_cols_array_2d(&sky_uniforms.inv_view_proj).inverse();

                // Cap a mote's per-step travel to ~one cell so the coarse grid can't
                // tunnel it; beat burst brightens the aura on the pulse.
                let cell = ((gmax - gmin) / Vec3::splat(res as f32)).max_element();
                let pulse = pulse_envelope(&s, self.beat_pos);
                let emissive = s.particles[7] * (1.0 + s.particles[11] * pulse);
                // #182 T2 beat coupling: the splash impulse rides the decaying
                // pulse envelope, active only while Pulse is on (like the voxel
                // beat pump).
                let fenv = if s.pulse != 0 { pulse } else { 0.0 };

                let key = (s.generator, count);
                let reseed = aura.key != key;
                aura.key = key;

                // Field energization (#247): light the motes by the energy density
                // baked into the grid's `w` channel. Two honest sources feed it:
                //  • Lite tier + Maxwell generator → the EM energy density ½(|E|²+|B|²)
                //    that `fill_analytic` wrote above (the fluorescent-tube demo).
                //  • Fluid tier (any generator) → the Navier–Stokes flow's own energy
                //    density ½|u|² + ½|ω|², baked into `w` by the solver's `cs_energy`
                //    pass (see `FluidParams::energize` below).
                // Same downstream glow either way; `energize = 0` → inert.
                let energize = s.maxenergy[0] != 0.0
                    && (fluid
                        || gen == GeneratorMode::MaxwellField
                        || gen == GeneratorMode::Acoustic);

                render::ParticlesFrame {
                    // `enabled` is the AURA's flag — false on the ink-only path
                    // (tier Off), which fills the grid fields but draws no motes.
                    enabled,
                    count,
                    grid_res: resv,
                    grid_min: gmin,
                    grid_max: gmax,
                    vel_grid: &aura.vel_upload,
                    nodes: &aura.node_samples,
                    view_proj,
                    cam_right,
                    cam_up,
                    dt: dt as f32,
                    time: self.gen_phase as f32,
                    frame_seed: aura.seed,
                    speed: s.particles[3],
                    lifetime: s.particles[4].max(0.05),
                    spawn_radius: s.particles[5],
                    drag: s.particles[12],
                    turbulence: s.particles[13],
                    max_step: cell.max(1e-3),
                    size: s.particles[6],
                    emissive,
                    alpha: s.particles[14],
                    ribbon: s.particles[8] > 0.5,
                    ribbon_stretch: s.particles[9],
                    hue_shift: s.particles[10],
                    energize,
                    energy_gain: s.maxenergy[1],
                    energy_knee: s.maxenergy[2],
                    energy_hue: s.maxenergy[3],
                    energy_contrast: s.mxforce[2],
                    energy_hue_cycle: self.hue_phase as f32,
                    reseed,
                    fluid,
                    fluid_params: render::FluidParams {
                        force: s.fluid[0],
                        vorticity: s.fluid[1],
                        dissipation: s.fluid[2],
                        iters: s.fluid[3] as u32,
                        inflow_decay: s.fluid[4],
                        boundaries: s.fluid2[0] != 0.0,
                        buoyancy: s.fluid2[1],
                        heat_decay: s.fluid2[2],
                        splash: s.fluid2[4],
                        beat_env: fenv,
                        substeps: (s.fluid2[7] as u32).clamp(1, 4),
                        // Bake the flow's energy density into vel.w only when the motes
                        // actually ride the fluid AND energization is on (the ink-only
                        // solve, tier Off, has no motes to light).
                        energize: energize && fluid,
                    },
                    // #298 Tier 1: shaded sphere-impostor beads.
                    beads: s.pbeads[0] != 0.0,
                    bead_metallic: s.pbeads[1],
                    bead_roughness: s.pbeads[2],
                    // #298 Tier 2: material / shape / ior / shape amount.
                    bead_material: s.pbeads[3] as u32,
                    bead_shape: s.pbeads[4] as u32,
                    bead_ior: s.pbeads[5],
                    bead_shape_param: s.pbeads[6],
                    // #305 Tier 1: bead material HSV (effective hue = base + cycle·beat).
                    bead_hue: s.pbeads2[0] + s.pbeads2[1] * self.beat_pos as f32,
                    bead_sat: s.pbeads2[2],
                    bead_val: s.pbeads2[3],
                    bead_emissive: s.emissive[2],
                }
            } else {
                // Disabled: a fully inert frame (the renderer early-outs on it).
                aura.key = (u32::MAX, 0);
                aura.prev_node_pos.clear();
                render::ParticlesFrame {
                    enabled: false,
                    count: 0,
                    grid_res: [1, 1, 1],
                    grid_min: Vec3::ZERO,
                    grid_max: Vec3::ONE,
                    vel_grid: &aura.vel_upload,
                    nodes: &aura.node_samples,
                    view_proj: Mat4::IDENTITY,
                    cam_right: Vec3::X,
                    cam_up: Vec3::Y,
                    dt: 0.0,
                    time: 0.0,
                    frame_seed: 0,
                    speed: 0.0,
                    lifetime: 1.0,
                    spawn_radius: 0.0,
                    drag: 0.0,
                    turbulence: 0.0,
                    max_step: 1.0,
                    size: 0.0,
                    emissive: 0.0,
                    alpha: 0.0,
                    ribbon: false,
                    ribbon_stretch: 0.0,
                    hue_shift: 0.0,
                    energize: false,
                    energy_gain: 1.0,
                    energy_knee: 4.0,
                    energy_hue: 0.08,
                    energy_contrast: 1.0,
                    energy_hue_cycle: 0.0,
                    reseed: false,
                    fluid: false,
                    fluid_params: render::FluidParams {
                        force: 0.0,
                        vorticity: 0.0,
                        dissipation: 0.0,
                        iters: 0,
                        inflow_decay: 0.0,
                        boundaries: false,
                        buoyancy: 0.0,
                        heat_decay: 0.0,
                        splash: 0.0,
                        beat_env: 0.0,
                        substeps: 1,
                        energize: false,
                    },
                    beads: false,
                    bead_metallic: 0.0,
                    bead_roughness: 0.0,
                    bead_material: 0,
                    bead_shape: 0,
                    bead_ior: 1.45,
                    bead_shape_param: 0.5,
                    bead_hue: 0.0,
                    bead_sat: 1.0,
                    bead_val: 1.0,
                    bead_emissive: s.emissive[2],
                }
            }
        };
        // Hide the generator geometry (particles[15]) — only when the aura, the
        // Fluid Ink, or the MLS-MPM liquid (#182) is actually running, so checking
        // it never blanks the whole scene. Medium-only / pure-pool view.
        let hide_generator =
            (particle_frame.enabled || ink_on || liq_on) && s.particles[15] != 0.0;

        // Fluid Ink (#182 Tier 1): dye injection + volumetric render params.
        // #182 T2 beat coupling: the dye gate fades injection toward pulse-gated
        // (1 = ink puffs only on the beat), and `detail` drives both the
        // render-time curl swirl and the solver's curl pass (`want_curl`).
        let ink_env = if s.pulse != 0 { pulse_envelope(&s, self.beat_pos) } else { 0.0 };
        let ink_gate = s.fluid2[5].clamp(0.0, 1.0);
        let ink_frame = render::InkFrame {
            enabled: ink_on,
            dye_src: &self.fluid_grids.dye_upload,
            dye: render::DyeParams {
                rate: s.fluidvis[1] * ((1.0 - ink_gate) + ink_gate * ink_env),
                dissipation: s.fluidvis[7],
                maccormack: s.fluidvis[9] != 0.0,
                want_curl: s.fluid2[3] > 0.0,
            },
            params: render::InkParams {
                extinction: s.fluidvis[3],
                scatter: s.fluidvis[4],
                emissive: s.fluidvis[5],
                anisotropy: s.fluidvis[6],
                steps: s.fluidvis[8],
                half_res: s.fluidvis[10] != 0.0,
                detail: s.fluid2[3],
                reveal: s.fluidvis[11],
            },
        };

        // MLS-MPM liquid (#182 Tier 3a): an invisible tank centred on the
        // SMOOTHED field centre (`cam_center` — a per-frame AABB would slosh
        // the pool constantly), with the generator's nodes as moving colliders.
        let liq_res = (s.liquid[2] as u32).clamp(16, 96);
        let liq_half = s.liquid[6].max(1.0);
        // Tank vertical offset (liquid2[0]): slide the volume off the centre —
        // pool the liquid below the generator, or float it above.
        let liq_center = self.cam_center + Vec3::Y * s.liquid2[0];
        let liq_min = liq_center - Vec3::splat(liq_half);
        let liq_max = liq_center + Vec3::splat(liq_half);
        if liq_nodes {
            let resv = [liq_res, liq_res, liq_res];
            if self.fluid_grids.occ.res != resv {
                self.fluid_grids.occ = math::VelGrid::new(resv, liq_min, liq_max);
            } else {
                self.fluid_grids.occ.min = liq_min;
                self.fluid_grids.occ.max = liq_max;
            }
            let n = (liq_res as usize).pow(3);
            self.fluid_grids.occ_upload.clear();
            self.fluid_grids.occ_upload.resize(n, Vec4::ZERO);
            let cell = (2.0 * liq_half / liq_res as f32).max(1e-4);
            let occ_r = (0.6 / cell).clamp(1.0, 2.5);
            for (i, p) in aura.node_samples.iter().enumerate() {
                let v = aura.node_vels.get(i).copied().unwrap_or(Vec3::ZERO);
                self.fluid_grids.occ.mark_occupancy(p.truncate(), v, occ_r, &mut self.fluid_grids.occ_upload);
            }
        } else {
            self.fluid_grids.occ_upload.clear();
        }
        // #247 Tier 3 (energy → liquid): when the Maxwell energization is injecting
        // energy dye, ALSO splat the ember glow — brightness by the local field energy,
        // tinted by the ember hue — into a FIELD_RES grid over the tank. The liquid's
        // resolve pass adds it to the surface albedo (HDR) and the isosurface glows in
        // the field PATTERN (bright at the antenna tips / dipole lobe, dark in the
        // nulls). Reuses the same gain/knee/hue/strength as the dye; needs no Fluid Ink
        // (the liquid renders it itself). `energize`/`strength` 0 → empty → byte-identical.
        let energy_liquid = liq_nodes
            && s.maxenergy[0] != 0.0
            && generator == GeneratorMode::MaxwellField
            && s.maxenergy[6] != 0.0;
        if energy_liquid {
            let fres = render::FIELD_RES;
            let resv = [fres, fres, fres];
            if self.fluid_grids.glow.res != resv {
                self.fluid_grids.glow = math::VelGrid::new(resv, liq_min, liq_max);
            } else {
                self.fluid_grids.glow.min = liq_min;
                self.fluid_grids.glow.max = liq_max;
                self.fluid_grids.glow.clear();
            }
            let ember = math::hsv_tint(s.maxenergy[3]).truncate();
            let dye_gain = s.maxenergy[1];
            let dye_knee = s.maxenergy[2];
            let dye_contrast = s.mxforce[2].max(0.05); // #248 near-core contrast
            let strength = s.maxenergy[6];
            // #248 Tier 2: colour by band for the liquid ember too (same blend as
            // the ink dye — bass holds the ember hue, highs pull across the wheel).
            let liq_band_elems = if audio_multipole_on(&s) && s.audiodip[5] > 0.0 {
                audio_band_elems(&s)
            } else {
                Vec::new()
            };
            // #248 Tier 3: waveform shells on the direct band-math path (the
            // grid-sampled path already carries them in `w`).
            let liq_wave = if s.audiodip[0] != 0.0 { s.audiodip2[0] } else { 0.0 };
            let liq_wave_span = 0.5 * (aura.vel_grid.max - aura.vel_grid.min).max_element();
            let liq_lean = audio_stereo_lean(&s);
            // A generous ball so the glow reads through the liquid volume, not just
            // at the node centres.
            let cell = (2.0 * liq_half / fres as f32).max(1e-4);
            let radius = (0.9 / cell).clamp(1.0, 4.0);
            let stride = (aura.node_samples.len() / MAX_DYE_NODES).max(1);
            for p in aura.node_samples.iter().step_by(stride) {
                let pw = p.truncate();
                let (col, mapped) = if liq_band_elems.is_empty() {
                    let u = aura.vel_grid.sample_energy(pw).powf(dye_contrast);
                    (ember, math::energy_tonemap(u, dye_gain, dye_knee))
                } else {
                    let (per, mut tot) = math::maxwell_band_energies(
                        pw,
                        &liq_band_elems,
                        s.maxenergy[7], // aura E↔B blend → glow follows the selected field
                        s.maxwell[7],
                        s.maxwell[10],
                        self.maxdip_phase as f32,
                    );
                    if liq_wave > 0.0 {
                        let r = (pw - Vec3::new(liq_lean, 0.0, 0.0)).length();
                        let g = math::radial_wave_gain(&self.rms_hist, liq_wave_span, r);
                        tot *= 1.0 + liq_wave.clamp(0.0, 1.0) * math::WAVE_BOOST * g;
                    }
                    let band =
                        math::hsv_tint(math::band_hue_blend(&per, s.maxenergy[3])).truncate();
                    (
                        ember.lerp(band, s.audiodip[5].clamp(0.0, 1.0)),
                        math::energy_tonemap(tot.powf(dye_contrast), dye_gain, dye_knee),
                    )
                };
                if mapped > 0.0 {
                    self.fluid_grids.glow.splat_ball(pw, col * (mapped * strength), radius);
                }
            }
            self.fluid_grids.glow.to_vec4(&mut self.fluid_grids.glow_upload, false);
        } else {
            self.fluid_grids.glow_upload.clear();
        }
        // Liquid albedo: hue around the wheel, desaturated toward white.
        let liq_rgb = {
            let hc = math::hsv_tint(s.liquid[12]);
            Vec3::ONE.lerp(Vec3::new(hc.x, hc.y, hc.z), s.liquid[13].clamp(0.0, 1.0))
        };
        let liquid_frame = render::LiquidFrame {
            enabled: liq_on,
            count: ((s.liquid[1].max(1.0) * 1000.0) as usize).min(render::MAX_LIQUID_PARTICLES),
            grid_res: liq_res,
            container_min: liq_min,
            container_max: liq_max,
            colliders: &self.fluid_grids.occ_upload,
            glow: &self.fluid_grids.glow_upload,
            dt: dt as f32,
            surface: render::MetaballParams {
                radius: 0.0, // unused — the density splat fills the field directly
                threshold: s.liquid[11].max(0.05),
                smoothness: 0.0,
                vol_density: 0.0,
                vol_emission: 0.0,
                vol_absorption: 0.0,
                steps: 0.0, // metaball default step budget
                band_coloured: 0.0, // liquid isosurface: no spectral colour
            },
            params: render::LiquidParams {
                gravity: s.liquid[3],
                stiffness: s.liquid[4],
                viscosity: s.liquid[5],
                open_top: s.liquid[7] != 0.0,
                collide: liq_nodes,
                stir: s.liquid[9],
                splat_scale: s.liquid[10],
                color: liq_rgb,
                substeps: (s.liquid[15] as u32).clamp(1, 4),
                shape: (s.liquid2[1] as u32).min(3),
                reveal: s.liquid2[2],
                reset_gen: s.liquid[14] as u32,
            },
            // #182 T4 follow-up: liqmat[0] 0 = follow the scene material;
            // 1..3 = the liquid's own Standard/Chrome/Glass (cube ids 0..2)
            // with the FULL dial set from liqmat/liqmat2. Decode by ROUNDING —
            // a CC/automation value between steps must snap to the nearest
            // variant, not truncate (0.7 as u32 - 1 would underflow → Glass).
            material: if s.liqmat[0].round().clamp(0.0, 3.0) as u32 >= 1 {
                Some(render::LiquidMaterial {
                    mat_type: (s.liqmat[0].round().clamp(0.0, 3.0) as u32) - 1,
                    metallic: s.liqmat[1].clamp(0.0, 1.0),
                    roughness: s.liqmat[2].clamp(0.0, 1.0),
                    ior: s.liqmat[3].max(1.0),
                    glow: s.liqmat[7].max(0.0),
                    chrome_purity: s.liqmat2[0],
                    glass_clarity: s.liqmat2[1],
                    f0_override: s.liqmat2[2],
                    dispersion: s.liqmat2[3],
                    glass_caustic: s.liqmat2[4],
                    thin_film: s.liqmat2[5],
                })
            } else {
                None
            },
            render_mode: (s.liqmat[5] as u32).min(1),
            absorb: s.liqmat[6].max(0.0),
        };

        // (#572 stage 3) The swapchain acquire that used to sit here — with its
        // Occluded/Lost handling, its reconfigure and its EDR re-assert — belongs to whoever
        // owns the surface, and runs *before* this call. By the time the frame body starts, the
        // texture it draws into is already alive and `target.hdr_max` already measured.
        // Global render resolution: smooth the frame time, then either hold the
        // manual scale or (Auto) steer it toward a 60 FPS target. The composite
        // upscales to the native swapchain, so the output stays full-res.
        let dt_ms = (dt * 1000.0) as f32;
        self.frame_ms += (dt_ms - self.frame_ms) * 0.1;
        let mut render_scale = if s.render_auto != 0 {
            self.auto_scale = drs_adjust(self.auto_scale, self.frame_ms, 1000.0 / 60.0);
            // Quantize with hysteresis (#174 T2): every DISTINCT scale value
            // rebuilds the whole render-target set (HDR + MSAA colour/depth +
            // AO/SSR/SSGI + the bloom chain + bind groups) — the smooth 25%-lerp
            // controller was recreating hundreds of MB of textures per frame for
            // 10–30 consecutive frames on every load change, exactly when the GPU
            // was already over budget (and could oscillate at the deadband edge).
            // Snap to 1/16 steps; move only once the smooth value strays ¾ of a
            // step from the applied one.
            if (self.auto_scale - self.applied_scale).abs() > 0.75 / 16.0 {
                self.applied_scale = ((self.auto_scale * 16.0).round() / 16.0).clamp(0.25, 1.0);
            }
            self.applied_scale
        } else {
            s.render_scale.clamp(0.25, 1.0)
        };
        // Form-resolution divisor (#127 P3): when an implicit minimal-surface family
        // is active (the per-pixel raymarch — Bubbles/Foam are the heavy ones), the
        // raymarch IS the scene, so an extra scale factor renders the whole pass at a
        // fraction and the composite upscales it — a quadratic perf lever for these
        // forms only, leaving every other generator at full resolution.
        if minimal {
            // The param's real range is 0.25–1.0; a value below that is the legacy
            // reserved zero (an older plugin's snapshot, or a pre-P3 preset) and must
            // mean "no extra downscale" (1.0), not 0.1 — otherwise mismatched
            // plugin/visual builds would silently quarter-res existing TPMS sessions.
            let fr = s.minimal_surface[14];
            let fr = if fr < 0.2 { 1.0 } else { fr.clamp(0.25, 1.0) };
            render_scale = (render_scale * fr).clamp(0.1, 1.0);
        }

        // Learned upscaler (#200 Tier 5c): the adaptive-sharpen reconstruction only
        // makes sense when the composite is actually upscaling. At (effectively)
        // full render scale there's nothing to reconstruct, so fall back to the
        // plain bilinear fetch — no surprise sharpening at 100%.
        if render_scale >= 0.999 {
            post_params.up_mode = 0.0;
        }

        // #174 T3: TAA sub-pixel jitter. A Halton-(2,3) offset (±½ px at the RENDER
        // resolution) rides the view-proj as a clip-space translation, so the depth
        // prepass and the scene rasterize identically jittered (the Equal test
        // holds) and the TAA history integrates true supersamples — without this,
        // TAA converged to the current frame's aliasing exactly (a temporal blur,
        // zero spatial AA). Only applied while TAA is on (no resolve → shimmer).
        // The temporal pass itself reprojects with the UNJITTERED matrices.
        self.frame_index = self.frame_index.wrapping_add(1);
        let unjittered_vp = uniforms.view_proj;
        let mut taa_jit = Mat4::IDENTITY;
        if s.temporal[0] != 0.0 {
            let rs = render::scaled_render_size(render_size, render_scale);
            let h = ((self.frame_index % 8) + 1) as u32; // Halton is 1-based
            let jx = (halton(h, 2) - 0.5) * 2.0 / rs.0.max(1) as f32;
            let jy = (halton(h, 3) - 0.5) * 2.0 / rs.1.max(1) as f32;
            taa_jit.w_axis.x = jx;
            taa_jit.w_axis.y = jy;
            uniforms.view_proj =
                (taa_jit * Mat4::from_cols_array_2d(&uniforms.view_proj)).to_cols_array_2d();
        }
        // Scenery view-proj (#187 composite fix). Pure ride: the rail camera IS
        // the scene camera — pass the scene view-proj through (unchanged look).
        // Composite: IDENTITY VIEW × the same projection (+ the same TAA
        // jitter) — the eye sits at the bore centre looking down −Z, so the
        // corridor is glued to the camera (always straight ahead/behind/around,
        // flying forward, never orbiting) while the generator is viewed by the
        // orbit rig. Both depths are eye-relative distances, so the corridor
        // composites honestly into the shared depth buffer and the prepass
        // reconstruction lands it at its true view positions (SSAO/SSR/DoF
        // coherent). Breath deliberately not applied (it scales the OBJECT).
        // Meander-facing camera (#206): when a Terra channel winds, yaw the
        // scenery view so the channel heads straight down −Z — the river rotates
        // underneath as it twists while the centred object floats down the middle
        // (the geometry zeroes the channel centre at `u_now`, but as you fly the
        // meander wanders and the eye still tracks it side to side). The follow
        // yaws about a LOOK-AHEAD pivot rather than the eye: the eye swings
        // laterally to counter the channel's horizontal offset — the whole scenery
        // slides under us so we stay centred on the near channel while facing down
        // the bend. Decays to 0 off Terra.
        let target_yaw = if scenery_on && s.scenery[0] as u32 == 2 {
            let spec = math::RailsSpec::from_slots(&self.scenery_active_blk[..24]);
            let terra = math::TerraSpec::from_slots(&self.scenery_active_blk[24..]);
            math::terra_channel_heading(&spec, &terra, self.beat_pos)
        } else {
            0.0
        };
        self.channel_yaw += (target_yaw - self.channel_yaw) * 0.15;
        // Pivot distance: how far ahead the channel centre we swing about (the
        // point that stays put while the eye slides sideways). Scales with bore.
        let pivot_d = (self.rails_bore * 3.0).max(8.0);
        let follow = Mat4::from_translation(Vec3::new(0.0, 0.0, -pivot_d))
            * Mat4::from_rotation_y(-self.channel_yaw)
            * Mat4::from_translation(Vec3::new(0.0, 0.0, pivot_d));
        let scenery_vp = if self.rails_ride {
            (Mat4::from_cols_array_2d(&uniforms.view_proj) * follow).to_cols_array_2d()
        } else {
            let aspect = (render_size.0 as f32 / render_size.1.max(1) as f32).max(0.01);
            let proj = Mat4::perspective_rh(45f32.to_radians(), aspect, CAM_NEAR, CAM_FAR);
            (taa_jit * proj * follow).to_cols_array_2d()
        };

        // The frame's output view. Everything downstream — the composite, the letterbox blit,
        // the overlay/HUD passes, the RT debug view — writes through this one view and has no
        // idea whether it is a swapchain image or a pane's texture. Since #572 stage 3 there is
        // only one arm here, which is the point: the two paths stopped being different.
        let view = target
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // #104: exactly one render path per frame (mutually exclusive by type).
        let path = if mandelbulb {
            render::RenderPath::Mandelbulb
        } else if creature_on {
            render::RenderPath::Creature
        } else if minimal {
            render::RenderPath::MinimalSurface
        } else if kifs_on {
            render::RenderPath::Kifs
        } else if neural_field_on {
            render::RenderPath::NeuralField
        } else if lens_on {
            render::RenderPath::Lens
        } else if metaball {
            render::RenderPath::Metaball
        } else if volume {
            render::RenderPath::Volume
        } else if voxel {
            render::RenderPath::Voxel
        } else if draw_membrane_mesh || (membrane_mode && membrane_arms) {
            // Skin-Arms uses the Membrane render path even though no shell sheet is
            // built (draw_membrane_mesh = false): the arm geometry — welded Mesh
            // (draw_swept) or capsule Impostors (draw_arms) — is drawn inside the
            // renderer's `membrane` branch, so the path must resolve to Membrane or
            // the Impostor arms draw nothing.
            render::RenderPath::Membrane
        } else if s.surface_mode == 8 && boids_creature < 0 {
            // Gaussian Splatting surface: the node set (built exactly like Original —
            // independent nodes, no flow-align) is drawn as anisotropic Gaussians. Like
            // metaball/voxel/volume it defers to Boids creatures (they own the instanced
            // creature meshes), so a creature form still renders as creatures.
            render::RenderPath::Splat
        } else {
            render::RenderPath::Instanced
        };
        // Temporal pass (#152 Tier 2): TAA + motion blur, with the current + previous
        // scene view-proj for camera-reprojection velocity. The pass runs only when
        // TAA or motion blur is on.
        let temporal_params = render::TemporalParams {
            enabled: s.temporal[0] != 0.0 || s.temporal[3] != 0.0,
            taa: s.temporal[0] != 0.0,
            motion_blur: s.temporal[3] != 0.0,
            taa_blend: s.temporal[1],
            taa_sharpen: s.temporal[2],
            mb_amount: s.temporal[4],
            mb_samples: s.temporal[5],
            // Unjittered matrices (#174 T3): reprojection velocity must not see the
            // sub-pixel jitter, or history sampling wobbles by the jitter delta.
            cur_view_proj: unjittered_vp,
            prev_view_proj: self.prev_view_proj,
        };
        // Lens flare anchor (#167 T1): project the key light — a directional source
        // at infinity — into screen space so the flare is tied to the ACTUAL light,
        // not to bright pixels. Its ghosts then sweep along the light→centre axis as
        // the camera orbits (camera-view × light-direction). Visibility fades as the
        // light leaves the frame / goes behind the camera and scales with key
        // intensity, so looking away from the light kills the flare.
        let (lf_light_x, lf_light_y, lf_visibility) = {
            let kd = Vec3::new(
                uniforms.key_light[0],
                uniforms.key_light[1],
                uniforms.key_light[2],
            );
            let vp = Mat4::from_cols_array_2d(&uniforms.view_proj);
            let clip = vp * kd.extend(0.0); // w = 0: a point at infinity (direction)
            if clip.w > 1e-4 {
                let ndc = clip.truncate() / clip.w;
                let lx = ndc.x * 0.5 + 0.5;
                let ly = 0.5 - ndc.y * 0.5;
                // Smoothly fade to 0 within a 0.25-uv margin past each frame edge.
                let edge = |t: f32| (1.0 - ((t - 0.5).abs() - 0.5).max(0.0) / 0.25).clamp(0.0, 1.0);
                let onscreen = edge(lx) * edge(ly);
                let intens = uniforms.key_light[3].clamp(0.0, 3.0);
                (lx, ly, onscreen * intens)
            } else {
                (0.5, 0.5, 0.0) // behind the camera → no flare
            }
        };
        // Post-composite creative FX (#152): built from Shared.fx; the FX pass only
        // runs when `enabled`. Grain animates on the wall clock.
        let fx_params = render::FxParams {
            enabled: s.fx[0] != 0.0,
            style: s.fx[1],
            style_amt: s.fx[2],
            dof: s.fx[3],
            dof_focus: s.fx[4],
            dof_range: s.fx[5],
            chroma: s.fx[6],
            vignette: s.fx[7],
            grain: s.fx[8],
            grade_sat: s.fx[9],
            grade_contrast: s.fx[10],
            grade_temp: s.fx[11],
            grade_gain: s.fx[12],
            feedback: s.fx[13],
            outline: s.fx[14],
            time: self.wall_time as f32,
            // Cinematic finishing (#167 T1): halation + lens flares (Shared.finishing).
            hal_amount: s.finishing[0],
            hal_threshold: s.finishing[1],
            hal_width: s.finishing[2],
            hal_warmth: s.finishing[3],
            lf_amount: s.finishing[4],
            lf_ghosts: s.finishing[5],
            lf_halo: s.finishing[6],
            lf_streak: s.finishing[7],
            lf_light_x,
            lf_light_y,
            lf_visibility,
            cam_near: CAM_NEAR,
            cam_far: CAM_FAR,
        };
        // Hardware RT (#195): rebuild the TLAS over this frame's instance matrices
        // (the whole field animates every frame → full rebuild, not a refit).
        // Tier 0's master toggle builds it bare (the cost measurement + debug
        // view); Tier 1's RT shadows imply the build too. Instanced path only,
        // like the shadow map. Runs BEFORE the frame is assembled so the shadow
        // pass can borrow the TLAS through `LightTransport`.
        // Fluid sway (#182 T4) displaces instances ON THE GPU after the TLAS is
        // built (no readback by design), so every traced effect would lag the
        // swayed geometry — all three fall back to their raster counterparts
        // while sway can be active (conservative: the dial up + a medium
        // running) — #198/#205 reviews. Tier 0's bare master toggle still
        // builds (it consumes nothing; tlas_ms stays honest).
        let sway_live = s.fluidgi[3] > 0.0 && (ink_on || liq_on || particle_frame.enabled);
        let rt_on = (s.rt[0] != 0.0
            || ((s.rt[2] != 0.0
                || s.rt2[0] != 0.0
                || (ssao_on && s.rt2[5] != 0.0)
                || s.rt3[0] != 0.0)
                && !sway_live))
            && matches!(path, render::RenderPath::Instanced)
            // Welded Swept Tubes empties `instances`; the ray tracer traces
            // `rt_instances` instead, so either being non-empty means there's
            // geometry for the TLAS.
            && (!self.geom.instances.is_empty() || !self.geom.rt_instances.is_empty())
            // Review fixes (#197): the TLAS must only cover what the raster
            // scene actually draws with the cube/cyl meshes. A hidden
            // generator (medium-only view) draws no instances, and a Boids
            // creature form draws per-agent creature meshes (no BLAS for
            // those yet) — building a TLAS there would make the debug view
            // lie and skew tlas_ms toward unsupported paths.
            && !hide_generator
            && boids_creature < 0;
        // RE-ENABLED on wgpu 30 (#195): wgpu 29's Metal backend wedged the
        // GPU machine-wide on ANY ray-query dispatch (a 1-ray/1-triangle
        // headless compute probe hung the queue — scratchpad `rqprobe`,
        // 2026-07-03). wgpu 30 fixed it upstream: the same probe returns
        // correct hits, and 300 frames of rebuild+query churn run clean.
        // If a machine-wide glitch EVER reappears, re-pin these to
        // false / false first and re-run the probe ladder.
        let rt_debug_on = rt_on && s.rt[1] != 0.0;
        // Path tracer (#200 Tier 4): wants the TLAS built under the same scene
        // conditions as `rt_on` (instanced geometry with a BLAS), independent of
        // the editor's RT card. `gfx.rt.is_some()` (ray-query support) is checked
        // where the frame is built below.
        // organon#217 T5: `pathtrace_active` is the raster → path-trace handover — the
        // preset's toggle OR a live, settled glyph frame. With no ring it IS the toggle.
        let pt_active = pathtrace_active(self.pathtrace_on, self.glyph_pt);
        let pt_want = pt_active
            && !hide_generator
            && boids_creature < 0
            && (
                // Instanced geometry — the TLAS the tracer queries. Welded Swept Tubes
                // empties `instances` (raster draws the welded mesh), so the tracer
                // uses `rt_instances`; either non-empty means there's geometry.
                (matches!(path, render::RenderPath::Instanced)
                    && (!self.geom.instances.is_empty() || !self.geom.rt_instances.is_empty()))
                // Lens (#258 T3): the tracer intersects the analytic lens directly
                // (it isn't in the TLAS) so it can focus light through it.
                || matches!(path, render::RenderPath::Lens)
            );
        let rt_shadows_live = true;
        // (Verified live on wgpu 30, 2026-07-03: debug view all 4 modes +
        // RT shadows key+fill at 10k instances — 120 fps, no faults.)
        let mut rt_tlas_ms = 0.0;
        if rt_on || pt_want {
            if let Some(rtc) = gfx.rt.as_mut() {
                // While ANY pass queries the TLAS, fully serialize the rebuild
                // against the in-flight queries (wgpu-hal Metal has no AS
                // barrier): wait out the previous frame's query before
                // rewriting the TLAS, and the build before this frame's query.
                let rt_query_live = rt_debug_on
                    || pt_want
                    || (rt_shadows_live
                        && !sway_live
                        && ((s.rt[2] != 0.0 && s.rt[4] > 0.0)
                            || (s.rt2[0] != 0.0 && s.rt2[1] > 0.0)
                            || (ssao_on && s.rt2[5] != 0.0)
                            || (s.rt3[0] != 0.0 && s.rt3[1] > 0.0)));
                if rt_query_live {
                    let _ = gfx
                        .device
                        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                }
                // Welded Swept Tubes: `self.geom.instances` is empty (the raster draws the
                // welded mesh), so trace the per-segment cylinder approximation.
                let rt_geo: &[Mat4] = if !self.geom.rt_instances.is_empty() {
                    &self.geom.rt_instances
                } else {
                    &self.geom.instances
                };
                // Plexus Tier-1 draws two morphed sub-batches (markers, struts), so the
                // single cube/cyl BLAS would mismatch RT. Approximate with the nearest
                // static BLAS per range — markers → sphere, struts → cylinder — which
                // matches the default (sphere / circle) shape exactly.
                if let Some(pb) = self.plexus.batches {
                    rtc.build_plexus(&gfx.device, &gfx.queue, rt_geo, pb.markers as usize);
                } else {
                    rtc.build(&gfx.device, &gfx.queue, rt_geo, tube);
                }
                if rt_query_live {
                    let _ = gfx
                        .device
                        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
                }
                rt_tlas_ms = rtc.build_ms;
            }
        }
        // RT shadows (#195 Tier 1): hand the renderer the TLAS + the shadow dials.
        // None (off / unavailable / strength 0) = the byte-identical default path.
        // `rt_shadows_live` (above) keeps this None until ray queries are healthy.
        let rt_shadow_frame = if rt_shadows_live
            && rt_on
            && s.rt[2] != 0.0
            && s.rt[4] > 0.0
            && !sway_live
        {
            gfx.rt.as_ref().and_then(|r| r.tlas()).map(|tlas| render::RtShadowFrame {
                tlas,
                key_strength: s.rt[4],
                fill_strength: if s.rt[5] != 0.0 { s.rt[4] } else { 0.0 },
                softness: s.rt[3],
                // Ray reach: comfortably past the whole field from any node.
                // Clamped finite — NaN/inf must never reach the traversal
                // hardware (#198 review; mirrors the debug view's clamp).
                t_max: {
                    let d = (bounds.max - bounds.min).length().max(1.0) * 4.0;
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
            })
        } else {
            None
        };
        // RT reflections (#195 Tier 2): the TLAS + the reflection dials. Shares
        // every gate the shadows have (query health, sway lag — a traced mirror
        // of pre-sway geometry would visibly disagree with the swayed scene).
        let rt_reflect_frame = if rt_shadows_live
            && rt_on
            && s.rt2[0] != 0.0
            && s.rt2[1] > 0.0
            && !sway_live
        {
            gfx.rt.as_ref().and_then(|r| r.tlas()).map(|tlas| render::RtReflectFrame {
                tlas,
                intensity: s.rt2[1],
                max_roughness: s.rt2[2],
                // Reach: a preset-captured multiple of the scene diagonal,
                // finite-clamped like the shadow reach.
                t_max: {
                    let d = (bounds.max - bounds.min).length().max(1.0)
                        * s.rt2[3].clamp(0.25, 8.0);
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
                hit_shadows: s.rt2[4] != 0.0,
                // The hit-shadow rays' own reach — Tier 1's scene formula,
                // independent of the reflection reach dial (#204 review).
                rays: (s.rt2[7] as u32).clamp(1, 16),
                shadow_t_max: {
                    let d = (bounds.max - bounds.min).length().max(1.0) * 4.0;
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
            })
        } else {
            None
        };
        // RT ambient occlusion (#195 Tier 3): the AO card's source switch. Same
        // gates as the other RT effects; None → the renderer runs GTAO instead
        // (graceful fallback on non-RT machines / while sway is live).
        let rt_ao_frame = if rt_shadows_live
            && rt_on
            && ssao_on
            && s.rt2[5] != 0.0
            && !sway_live
        {
            gfx.rt.as_ref().and_then(|r| r.tlas()).map(|tlas| render::RtAoFrame {
                tlas,
                radius: s.ssao[1].max(1e-3), // the shared AO radius dial
                rays: (s.rt2[6] as u32).clamp(1, 16),
            })
        } else {
            None
        };
        // RT diffuse GI (#195 Tier 4): the TLAS + the gather dials. Same gates as
        // the other RT effects; None → the renderer runs SSGI instead (graceful
        // fallback on non-RT machines / while sway is live).
        let rt_gi_frame = if rt_shadows_live
            && rt_on
            && s.rt3[0] != 0.0
            && s.rt3[1] > 0.0
            && !sway_live
        {
            gfx.rt.as_ref().and_then(|r| r.tlas()).map(|tlas| render::RtGiFrame {
                tlas,
                intensity: s.rt3[1],
                rays: (s.rt3[2] as u32).clamp(1, 16),
                // Gather reach: a preset-captured multiple of the scene
                // diagonal, finite-clamped like the other RT reaches.
                t_max: {
                    let d = (bounds.max - bounds.min).length().max(1.0)
                        * s.rt3[3].clamp(0.25, 8.0);
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
                hit_shadows: s.rt3[4] != 0.0,
                // The hit-shadow rays' own reach — Tier 1's scene formula,
                // independent of the GI reach dial (mirrors #204 for reflections).
                shadow_t_max: {
                    let d = (bounds.max - bounds.min).length().max(1.0) * 4.0;
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
            })
        } else {
            None
        };
        // Path tracer (#200 Tier 4): camera-still detection → progressive sample
        // count. Any camera move (exact compare of the unjittered VP) restarts the
        // accumulation; a held camera (pause the animation with Speed 0 for a clean
        // converged reference — the TLAS rebuilds each frame, so a moving field
        // would smear the average) integrates 1 spp/frame toward ground truth.
        // Restart on a camera move OR when the accumulation buffers are resized
        // (render-scale change / window resize recreate them, so a stale sample
        // count would blend against wrong-resolution history — #234 review).
        let pt_size = render::scaled_render_size(render_size, render_scale);
        let pt_moved = unjittered_vp != self.pt_prev_vp;
        let pt_resized = pt_size != self.pt_prev_size;
        // Restart accumulation when a setting that changes what the buffer holds
        // flips (else the old-mode samples ghost as a frozen "after image").
        // (#258 T5: the caustic Look dials change what the accumulation holds too —
        // the photon BUDGET doesn't, it only trades per-frame variance, so it's out.)
        // organon#217 T5: the key also carries the glyph ring's `(live, generation)` —
        // the one geometry counter that is safe to restart on, because the producer
        // bumps it only when the cell payload changes and holds it through the dwell's
        // heartbeat; `pt_content_key`'s doc says why that is the exception to "a moving
        // field would smear the average". And the restart is keyed on `pt_active`, not
        // the preset toggle: while the ring is in motion the tracer is off and the count
        // is held at 0, so the dwell's first traced frame starts clean.
        let pt_content = pt_content_key(&s, self.glyph_pt);
        let pt_content_changed = pt_content != self.pt_prev_content;
        if pathtrace_restarts(pt_moved, pt_resized, pt_content_changed, pt_active) {
            self.pathtrace_spp = 0;
        }
        self.pt_prev_vp = unjittered_vp;
        self.pt_prev_size = pt_size;
        self.pt_prev_content = pt_content;

        // #256 Tier 0 — the live radiance cache. With the path tracer on and the cache
        // enabled, train the #200 Tier-6 SIREN a few samples/frame and snapshot its
        // weights for the tracer's early-termination query. TARGET (bake-first path):
        // the analytic environment radiance the tracer's miss shader returns
        // (`math::nrc_sky_radiance`, mirroring `rt_pathtrace.wgsl::sky`), sampled at
        // random field points + sphere directions — a real, smooth, learnable light
        // field the cache converges to as its cold-start ambient prior. (Position-
        // dependent bounced GI from the tracer's own extra-bounce samples is the
        // on-Mac online-training follow-up — it needs a GPU→CPU radiance readback this
        // env has no way to run.) The confidence blend in the shader keeps the render
        // safe regardless of how well-trained the cache is.
        // Train whenever the cache is enabled AND something consumes it — the path
        // tracer (early-termination / guiding) OR cache GI (the raster probe grid).
        let nrc_active = s.nrc[0] > 0.5 && (pt_want || s.nrc3[0] > 0.5);
        let nrc_omega = s.nrc[3].clamp(0.1, 32.0);
        let mut nrc_weights: Vec<f32> = Vec::new();
        if nrc_active {
            let seed = (s.nrc[6].max(1.0)) as u32;
            // Rebuild the cache on a seed / frequency change (a fresh network).
            if self.nrc_cache.is_none() || self.nrc_key != (seed, nrc_omega.to_bits()) {
                self.nrc_cache = Some(organon_core::math::RadianceCache::new(seed, nrc_omega));
                self.nrc_key = (seed, nrc_omega.to_bits());
                self.nrc_loss = 0.0;
                self.nrc_state = 1;
            }
            let bmin = [bounds.min.x, bounds.min.y, bounds.min.z];
            let bmax = [bounds.max.x, bounds.max.y, bounds.max.z];
            let env_int = uniforms.env[1];
            let tint = [uniforms.env_tint[0], uniforms.env_tint[1], uniforms.env_tint[2]];
            let key_dir = [uniforms.key_light[0], uniforms.key_light[1], uniforms.key_light[2]];
            let key_int = uniforms.key_light[3];
            let lr = s.nrc[2].clamp(0.0, 1.0);
            let n = s.nrc[5].clamp(0.0, 256.0) as u32;
            if let Some(cache) = self.nrc_cache.as_mut() {
                // Deterministic per-frame sample batch (reproducible via the sample count).
                let fseed = self.pathtrace_spp.wrapping_mul(2_654_435_761).wrapping_add(seed);
                let mut batch: Vec<([f32; organon_core::math::NRC_IN], [f32; organon_core::math::NRC_OUT])> =
                    Vec::with_capacity(n as usize);
                for k in 0..n {
                    let r = |c: u32| organon_core::math::mlp_rand(fseed.wrapping_add(k.wrapping_mul(7)), c);
                    let p = [
                        bmin[0] + (r(0) * 0.5 + 0.5) * (bmax[0] - bmin[0]),
                        bmin[1] + (r(1) * 0.5 + 0.5) * (bmax[1] - bmin[1]),
                        bmin[2] + (r(2) * 0.5 + 0.5) * (bmax[2] - bmin[2]),
                    ];
                    let dv = [r(3), r(4), r(5)];
                    let dl = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt().max(1e-4);
                    let d = [dv[0] / dl, dv[1] / dl, dv[2] / dl];
                    let x = organon_core::math::RadianceCache::encode(p, bmin, bmax, d);
                    let t = organon_core::math::nrc_sky_radiance(d, env_int, tint, key_dir, key_int);
                    batch.push((x, t));
                }
                if n > 0 {
                    let loss = cache.train_batch(&batch, lr);
                    self.nrc_loss = if self.nrc_loss <= 0.0 { loss } else { self.nrc_loss * 0.9 + loss * 0.1 };
                    self.nrc_state = if self.nrc_loss < 0.02 { 2 } else { 1 };
                } else {
                    // No samples this frame: an empty batch's mean loss is 0.0, which
                    // would falsely read as "converged" while the weights stay cold.
                    // Keep the cache queryable (upload below) but report warming.
                    self.nrc_state = 1;
                }
                nrc_weights = cache.w.to_vec();
            }
        } else {
            self.nrc_loss = 0.0;
            self.nrc_state = 0;
        }

        // #256 T2 — cache-GI probe fill (deferred from the probe block above so it sees
        // THIS frame's trained cache). Placement spans the GI light volume; the encode
        // uses the cache's training AABB (`bounds`) so the normalized lookup coordinates
        // match what the cache was trained + queried with. `nrc_gi_on` implies the
        // training block above ran (nrc_active), so the cache is Some.
        if nrc_gi_on {
            if let Some(cache) = self.nrc_cache.as_ref() {
                gi_probes = math::compute_gi_probes_from_cache(
                    cache, light_min, light_max, bounds.min, bounds.max,
                );
            }
        }

        let pathtrace_frame = if pt_want {
            gfx.rt.as_ref().and_then(|r| r.tlas()).map(|tlas| render::PathtraceFrame {
                tlas,
                spp: self.pathtrace_spp,
                bounces: 4,
                reach: {
                    let d = (bounds.max - bounds.min).length().max(1.0) * 4.0;
                    if d.is_finite() { d.min(1e6) } else { 1e5 }
                },
                frame: self.pathtrace_spp,
                unjittered_view_proj: unjittered_vp,
                // #258 T2: dielectric BTDF enable + Beer–Lambert absorption.
                pt_dielectric: s.ptglass[0] > 0.5,
                pt_absorb: s.ptglass[1],
                // Composite mode + augment amount (ptglass[2]/[3]): Replace / Blend / GI-add.
                pt_composite: s.ptglass[2].round().clamp(0.0, 2.0) as u32,
                pt_augment: s.ptglass[3],
                // Analytic lens (#258 T3): derive the same CSG geometry `lens.wgsl`
                // raymarches (r/dz/aperture from focal/thickness/scale) so the tracer
                // intersects an identical body and focuses light through it.
                lens: if matches!(path, render::RenderPath::Lens) {
                    let lp = &lens_params;
                    let scale = lp.scale.max(1e-3);
                    let r = lp.focal * scale;
                    let aper = lp.aperture.clamp(0.02, 1.5) * scale;
                    let t = (lp.thickness.clamp(0.01, 0.98) * scale).min(r * 0.98);
                    let dz = r - t;
                    let plano = if lp.plano { 1.0 } else { 0.0 };
                    [lp.center.x, lp.center.y, lp.center.z, 1.0, r, dz, aper, plano]
                } else {
                    [0.0; 8]
                },
                // Spectral dispersion (#258 T4): [spectral_on, abbe, secondaries, _].
                spectral: [s.spectral[0], s.spectral[1], s.spectral[2], 0.0],
                // Photon-mapped caustics (#258 T5): [enable, photons (absolute count
                // from the ×1k dial), intensity, gather radius px].
                caustic: [
                    s.ptcaustic[0],
                    (s.ptcaustic[1] * 1000.0).clamp(1024.0, 2_097_152.0),
                    s.ptcaustic[2],
                    s.ptcaustic[3],
                ],
                // Photon emission disc: the framing bounds' sphere (the Lens path's
                // bounds already cover the lens body).
                scene_sphere: {
                    let c = (bounds.min + bounds.max) * 0.5;
                    let mut r = (bounds.max - bounds.min).length() * 0.5;
                    if !r.is_finite() || r <= 1e-3 { r = 10.0; }
                    [c.x, c.y, c.z, r.min(1e5)]
                },
                // World size of one traced pixel at unit distance (the deposit
                // footprint): 2·tan(fovy/2)/height at the tracer's render size.
                pixel_scale: 2.0 * (22.5f32).to_radians().tan() / pt_size.1.max(1) as f32,
                // #256 T0 — live radiance cache early-termination + weight upload.
                // [enable, confidence, omega, terminate_bounce]; weights + AABB match
                // the CPU training above. Off → [0;4] + empty weights (byte-identical).
                nrc: if nrc_active {
                    [1.0, s.nrc[1].clamp(0.0, 1.0), nrc_omega, s.nrc[4].max(1.0)]
                } else {
                    [0.0; 4]
                },
                nrc_weights: &nrc_weights,
                nrc_bbox_min: [bounds.min.x, bounds.min.y, bounds.min.z],
                nrc_bbox_max: [bounds.max.x, bounds.max.y, bounds.max.z],
                // #256 T1 — guided sampling + firefly clamp, armed only when the cache
                // is live (else off → the tracer's bounce/sample are unchanged).
                nrc1: if nrc_active {
                    [
                        if s.nrc2[0] > 0.5 { 1.0 } else { 0.0 },
                        s.nrc2[1].max(1.0),
                        if s.nrc2[2] > 0.5 { 1.0 } else { 0.0 },
                        s.nrc2[3].max(0.0),
                    ]
                } else {
                    [0.0; 4]
                },
                // #256 T2 — cache-lit reflections (needs the dielectric tracer + cache).
                nrc_reflect: nrc_active && s.nrc3[2] > 0.5,
                // #256 T3 — volumetrics + cached caustics (armed only with the cache live).
                nrc_volume: if nrc_active {
                    [
                        if s.nrc4[0] > 0.5 { 1.0 } else { 0.0 },
                        s.nrc4[1].max(0.0),
                        s.nrc4[2].max(1.0),
                        s.nrc4[3].max(0.0),
                    ]
                } else {
                    [0.0; 4]
                },
                nrc_caustic: if nrc_active {
                    [if s.nrc4[4] > 0.5 { 1.0 } else { 0.0 }, s.nrc4[5].max(0.0), 0.0, 0.0]
                } else {
                    [0.0; 4]
                },
            })
        } else {
            None
        };
        // Advance the sample count for next frame (this frame's sample is `spp`).
        if pathtrace_frame.is_some() {
            self.pathtrace_spp = self.pathtrace_spp.saturating_add(1);
        }
        // Beat-aware temporal accumulator (#200 Tier 4½ part 3): reproject +
        // accumulate the RT reflection/GI buffers over time to integrate the
        // 1-Nspp grain out. Gated only on the toggle + RT being live — the
        // renderer only actually invokes it inside the RT reflect/GI blocks, so
        // it's inert when neither runs. `beat_relax` folds the live PLL beat
        // envelope in (0 when pulse is off), dropping history weight on the kick
        // so history doesn't smear across the fast auto-orbit camera.
        let rt_temporal_frame = if rt_on && s.rt4[0] != 0.0 {
            let beat = if s.pulse != 0 { pulse_env } else { 0.0 };
            Some(render::RtTemporalFrame {
                cur_view_proj: unjittered_vp,
                prev_view_proj: self.prev_view_proj,
                feedback: s.rt4[1],
                beat_relax_factor: (beat as f32) * s.rt4[2],
                // Part 4: variance-guided SVGF (adaptive blend + σ-clamp).
                variance: s.rt4[3] != 0.0,
                max_accum: s.rt4[4].max(1.0),
                clamp_gamma: s.rt4[5].max(0.0),
            })
        } else {
            None
        };
        let render_frame = render::RenderFrame {
            size: render_size,
            render_scale,
            uniforms: &uniforms,
            sky_uniforms: &sky_uniforms,
            post_params: &post_params,
            fx: fx_params,
            temporal: temporal_params,
            kaleido: kaleido_params,
            ink: ink_frame,
            liquid: liquid_frame,
            coupling: render::CouplingFrame {
                gi: s.fluidgi[0],
                shadow: s.fluidgi[1],
                // Geometry shades the smoke only when the shadow map is real
                // (cast shadows on — otherwise the map is stale/empty).
                receive: s.fluidgi[2] != 0.0 && shadow_on,
                sway: s.fluidgi[3],
                caustic: s.caustic[0],
                caustic_sharp: s.caustic[1].max(0.1),
                ghost: s.liqmat[4] != 0.0,
            },
            background: render::Background {
                terrain_on,
                terrain_u: &terrain_u,
                terrain_scale: (s.terrain[16] as u32).max(1), // resolution divisor (1/2/4)
                stars_on,
                star_sun,
                star_u: &star_u,
                ocean_on,
            },
            surface: render::Surface {
                path,
                instances: &self.geom.instances,
                tints: &self.geom.tints,
                // organon#217 T1: empty on every frame the glyph ring is not driving.
                emits: &self.geom.emits,
                // Welded Swept Tubes: the RT/PT shade the per-segment cylinders while
                // the raster draws the welded mesh. Empty otherwise (RT uses instances).
                rt_instances: &self.geom.rt_instances,
                rt_tints: &self.geom.rt_tints,
                tube, // Swept Tubes / membrane strands → cylinders
                // Neural Tissue (#260 Tier 1): soma/capsule/bouton sub-batches for the
                // Neural Network generator; else None. `neural_capsule` renders other
                // generators' bridges as closed capsules under the Neural Tissue surface.
                neural_batches: self.neural_batches,
                neural_capsule: neural_tissue && self.neural_batches.is_none(),
                swept: !self.geom.swept_mesh.is_empty(), // Contiguous Swept Tubes welded mesh
                swept_verts: &self.geom.swept_mesh.verts,
                swept_idx: &self.geom.swept_mesh.indices,
                creature: boids_creature, // Boids creature mesh (#52): -1 none
                mem_pos: &self.mem_pos,
                mem_norm: &self.mem_norm,
                mem_col: &self.mem_col,
                mem_idx: &self.mem_idx,
                show_strands,
                membrane_arms,
                arm_caps: &self.arm_caps,
                plexus_impostor: plexus && s.plexus2[0] != 0.0,
                plexus_node_caps: &self.plexus.node_caps,
                plexus_edge_caps: &self.plexus.edge_caps,
                plexus_node_mat: render::PlexMat {
                    material: s.plexus_node_mat[0],
                    metallic: s.plexus_node_mat[1],
                    roughness: s.plexus_node_mat[2],
                    ior: s.plexus_node_mat[3],
                    glow: s.plexus_node_mat[7], // emissive → capsule params.y
                    hsv: Vec4::new(s.plexus_node_mat[4], s.plexus_node_mat[5], s.plexus_node_mat[6], 0.0),
                },
                plexus_edge_mat: render::PlexMat {
                    material: s.plexus_edge_mat[0],
                    metallic: s.plexus_edge_mat[1],
                    roughness: s.plexus_edge_mat[2],
                    ior: s.plexus_edge_mat[3],
                    glow: s.plexus_edge_mat[7],
                    hsv: Vec4::new(s.plexus_edge_mat[4], s.plexus_edge_mat[5], s.plexus_edge_mat[6], 0.0),
                },
                // organon#217 T6/T3: the coaxial capsule core, straight off the chain.
                capsule_core: [s.capsule[0], s.capsule[1]],
                plexus_batches: self.plexus.batches,
                plexus_node_verts: &self.plexus.node_mesh.verts,
                plexus_node_idx: &self.plexus.node_mesh.indices,
                plexus_edge_verts: &self.plexus.edge_mesh.verts,
                plexus_edge_idx: &self.plexus.edge_mesh.indices,
                plexus_overlay_batches: self.plexus.overlay_batches,
                plexus_ov_insts: &self.plexus.ov_insts,
                plexus_ov_tints: &self.plexus.ov_itints,
                meta_nodes: &self.meta_nodes,
                meta_min,
                meta_max,
                meta_params: &meta_params,
                field_vol_grid: &self.field_vol_grid,
                voxel_params: &voxel_params,
                mandel_params: &mandel_params,
                creature_params: &creature_params,
                minimal_params: &minimal_params,
                kifs_params: &kifs_params,
                neural_field_params: &neural_field_params,
                lens_params: &lens_params,
                // Gaussian Splatting surface look (Shared.splat). Only consumed when
                // path == Splat; reuses `instances`/`tints` as the splat cloud.
                splat_params: render::SplatParams {
                    radius: s.splat[0],
                    opacity: s.splat[1],
                    falloff: s.splat[2],
                    cutoff: s.splat[4],
                    aniso: s.splat[5],
                    scatter: s.splat[6].round().max(1.0) as u32,
                    jitter: s.splat[7],
                    solid: s.splat2[0],
                    lit: s.splat[3] >= 0.5,
                },
                particles: &particle_frame,
                hide_generator,
                axes_solids: &self.axes_solids,
                box_lines: &self.box_lines,
                chamber_surfs: &self.chamber.surfs,
                chamber_lines: &self.chamber.lines,
                chamber_beads: &self.chamber.beads,
                chamber_cam_right: self.chamber.cam_right,
                chamber_cam_up: self.chamber.cam_up,
                chamber_material: self.chamber.material,
                chamber_opacity: self.chamber.opacity,
                // Scenery layer (#187 pivot): the concurrent corridor with its
                // own material (patched into a second uniform set). Skin (#206
                // Tier 1) draws the lofted membrane instead of the instances.
                scenery: if scenery_on
                    && (!self.scenery_instances.is_empty() || !self.scenery_mem_idx.is_empty())
                {
                    Some(render::SceneryLayer {
                        instances: &self.scenery_instances,
                        tints: &self.scenery_tints,
                        tube: scenery_tube,
                        mem_pos: &self.scenery_mem_pos,
                        mem_norm: &self.scenery_mem_norm,
                        mem_col: &self.scenery_mem_col,
                        mem_idx: &self.scenery_mem_idx,
                        mat_type: s.scenery[2],
                        metallic: s.scenery[3],
                        roughness: s.scenery[4],
                        glow: s.scenery[5],
                        emissive: s.emissive[1],
                        opacity: s.scenery[6],
                        ior: s.scenery[7].max(1.0),
                        // Any explicit LUT ⇒ the tint IS the albedo; Native (0)
                        // keeps the HSV sweep as tint over the RGB cube. (The
                        // membrane always uses its per-vertex lofted colour.)
                        palette_active: if s.scenery[8] >= 0.5 { 1.0 } else { 0.0 },
                        sss: [s.scenery[9], s.scenery[10], s.scenery[11]],
                        irid: [s.scenery[12], s.scenery[13], s.scenery[14]],
                        // #305 T1: scenery material HSV (effective hue = base + cycle·beat).
                        matcol: [s.matcol[4] + s.matcol[5] * self.beat_pos as f32, s.matcol[6], s.matcol[7], 0.0],
                        view_proj: scenery_vp,
                        view_locked: !self.rails_ride,
                    })
                } else {
                    None
                },
                // Scenery water floor (#206 Tier 3): the channel water, lofted as
                // its own membrane with its own (glass) material. Present only
                // when Terra water produced a sheet this frame.
                water: if scenery_on && !self.water_mem_idx.is_empty() {
                    Some(render::WaterLayer {
                        mem_pos: &self.water_mem_pos,
                        mem_norm: &self.water_mem_norm,
                        mem_col: &self.water_mem_col,
                        mem_idx: &self.water_mem_idx,
                        mat_type: s.water[0],
                        roughness: s.water[1],
                        ior: s.water[2].max(1.0),
                        opacity: s.water[3],
                        glow: s.water[4],
                        // #305 T1: the SCENERY material's HSV (matcol[4..8]) — same as
                        // the scenery layer — so the water floor doesn't inherit the
                        // generator's hue/beat-cycle (Bugbot).
                        matcol: [
                            s.matcol[4] + s.matcol[5] * self.beat_pos as f32,
                            s.matcol[6],
                            s.matcol[7],
                            0.0,
                        ],
                        // Physical-water params (#206 dedicated water material).
                        absorb: s.water2[0],
                        glitter: s.water2[1],
                        reflect: s.water2[2],
                        view_proj: scenery_vp,
                    })
                } else {
                    None
                },
                demo_batches: &self.demo_batches,
            },
            light: render::LightTransport {
                ssao_on,
                ssao: &ssao,
                ssr_on,
                ssr: &ssr,
                ssgi_on,
                ssgi: &ssgi,
                gi_on,
                gi_intensity,
                gi_falloff,
                gi_min: light_min,
                gi_max: light_max,
                gi_probes: &gi_probes,
                rd_params: &rd_params,
                shadow_on,
                shadow_light_vp,
                shadow_bias: s.shadow[1],
                shadow_strength: s.shadow[2],
                vxgi_on,
                vxgi: render::VxgiParams {
                    intensity: s.vxgi[1],
                    rays: s.vxgi[2],
                    steps: s.vxgi[3],
                    // Reach: a fraction of the scene diagonal, so it reads consistently
                    // across generators of very different scale.
                    max_dist: (bounds.max - bounds.min).length().max(1e-3) * 0.5,
                    // #163 Tier 3: specular reflection cone. Reach scales the scene
                    // diagonal by the reach fraction; strength 0 → the cone is skipped.
                    spec_strength: s.vxgi_spec[0],
                    spec_aperture: s.vxgi_spec[1],
                    spec_max_dist: (bounds.max - bounds.min).length().max(1e-3) * s.vxgi_spec[2],
                    spec_steps: s.vxgi_spec[3],
                },
                // Emissive cubes as real lights (#167 Tier 3): brightest N nodes → point
                // lights the cube shader loops. Radius is a fraction of the scene diagonal.
                ml_on: manylight_on,
                ml_intensity: s.manylight[1],
                // organon#217 T10: in COLUMN WIDTHS while a glyph ring is live (§5.1 —
                // every glyph length is in cell units), converted against the same
                // bounds the renderer scales the fraction back by; the lane itself with
                // no ring. ⚠️ The renderer still multiplies the uploaded radiance by its
                // `radiance_scale` (`glow + 0.3·key`, `render.rs`): a glyph light's
                // colour is already radiance, so that factor is the one thing between
                // the preset's `ml_intensity` and SDR-white units — `render.rs`, W8.
                ml_radius: glyph_light_radius_frac(s.manylight[2], self.glyph_pt.live, glyph_look.cell_w, light_min, light_max),
                ml_count: s.manylight[3] as i32,
                // ReSTIR many-lights (#200 Tier 5d): reservoir importance sampling
                // of the emissive-cube light set. restir[0] = enable.
                ml_restir: s.restir[0] != 0.0,
                // Hardware-RT shadows (#195 Tier 1).
                rt_shadow: rt_shadow_frame,
                rt_reflect: rt_reflect_frame,
                rt_ao: rt_ao_frame,
                rt_gi: rt_gi_frame,
                rt_temporal: rt_temporal_frame,
                // RT denoise (#200 Tier 4½ part 2): amount, 0 = off. Only bites the
                // RT-written reflection/GI buffers (the renderer gates on those).
                rt_denoise: if s.rt3[5] != 0.0 { s.rt3[6].clamp(0.0, 1.0) } else { 0.0 },
                // Neural denoiser (#200 Tier 5a): when enabled, the denoise step
                // routes through the kernel-predicting filter (net = 0 reproduces
                // the classical à-trous). ndenoise = [enable, net, seed, omega, …].
                rt_ndenoise: if s.ndenoise[0] != 0.0 {
                    Some(render::NDenoiseFrame {
                        net: s.ndenoise[1].clamp(0.0, 1.0),
                        seed: s.ndenoise[2].max(0.0),
                        omega: s.ndenoise[3],
                    })
                } else {
                    None
                },
                // Membrane screen-space FX opt-in (draws the membrane into the prepass).
                membrane_fx: s.membrane_fx[0] != 0.0,
                // Path tracer (#200 Tier 4): the ground-truth override. When
                // `Some`, the renderer traces the whole image into the HDR scene
                // buffer (over the raster scene) and its `screen_geo = false` gate
                // skips the screen-space light passes; the composite adds no SSR/
                // SSGI (both inactive without the prepass), so bloom + tone-map
                // apply to the traced result cleanly.
                pathtrace: pathtrace_frame,
                // Screen-space refraction (#214 T5 pt 2): strength + displacement.
                // Gated render-side to the Refractive material + a valid prepass.
                refract_ss: s.ssrefr[0].clamp(0.0, 1.0),
                refract_dist: s.ssrefr[1].max(0.0),
            },
        };
        // Production frame (#135): render at the fixed output size into the capture
        // texture, then letterbox-blit it into the window. Native (cap_out = None)
        // renders straight to the swapchain as before. The frame's `out_format` tracks the
        // HDR toggle (sRGB8 ↔ Rgba16Float) — both the prod texture and the blit
        // pipeline follow it, so EDR content survives the blit (pure pass-through).
        // CPU-side render cost (#277 Tier 2): time the encode + submit of every
        // pass (this grows as GPU features toggle on — more passes = more encode).
        // The call returns before the GPU finishes, so this is CPU time, not the
        // frame's GPU time (that's Tier 3's timestamp query). Smoothed like frame_ms.
        // GPU frame timing (#277 Tier 3): progress any pending readback, then
        // bracket the render work with an opening timestamp submitted before it.
        if let Some(t) = gfx.gpu_timer.as_mut() {
            t.poll(&gfx.device);
            t.begin(&gfx.device, &gfx.queue);
        }
        let cpu_t0 = Instant::now();
        // #430: while recording, force the production-texture path even in Native aspect
        // (render into the offscreen at window size), so the recorder always has a
        // pixel-exact source to read back. The blit to the window is unchanged.
        // #452 Tier 3: a pending `snap` also forces the production-texture path (even in
        // Native aspect, not recording) so there is always a pixel-exact source to read
        // back — exactly what recording does.
        let record = &mut self.record;
        let cap_eff = cap_out
            .or_else(|| record.recorder.is_some().then_some(size))
            .or_else(|| self.cmd_chan.snap_pending.is_some().then_some(size));
        if let Some(out) = cap_eff {
            let prod_view = gfx.capture.production_view(&gfx.device, out, out_format);
            gfx.renderer.render(&gfx.device, &gfx.queue, &prod_view, &render_frame);
            // Read the production texture back into the encoder BEFORE the letterbox blit
            // — the blit is a display-only fit into the window; the recording is the
            // production frame itself (frame-exact, no letterbox bars, no display map).
            // Presented frames only (#582): the mirror's offscreen pass renders through here
            // too, and its production texture is the mirror's size — feeding it would interleave
            // 640×360 frames into a 1100×760 take.
            if let Some(rec) = record.recorder.as_mut().filter(|_| presented) {
                if let Some(tex) = gfx.capture.production_tex() {
                    // #430 chunk mode paces off MUSICAL time: the frame the file owes is
                    // derived from how far the beat clock has advanced past this clip's
                    // boundary, so the file's timeline is the song's timeline and the cuts
                    // can't slide against the grid. Plain takes pace off the wall clock.
                    let ideal = record.chunk_armed.then(|| {
                        let phrase = record.chunk_phrase_beats.max(1.0);
                        let anchor = record.chunk_grid_offset + record.chunk_index as f64 * phrase;
                        recorder::ideal_frame(
                            self.beat_pos - anchor,
                            record.chunk_bpm,
                            record.fps.value(),
                        )
                    });
                    rec.capture(&gfx.device, &gfx.queue, tex, ideal);
                }
            }
            // #452 Tier 3: service a pending single-frame snapshot off the SAME production
            // texture (a one-shot blocking readback → PNG), then reply with the path.
            if let Some((nonce, path)) = self.cmd_chan.snap_pending.take() {
                let res = match gfx.capture.production_tex() {
                    Some(tex) => snap::capture_png(&gfx.device, &gfx.queue, tex, &path)
                        .map_err(|e| e.to_string()),
                    None => Err("no production texture to snapshot".to_string()),
                };
                append_eyes_reply(&nonce, &res);
            }
            gfx.capture.blit_letterbox(
                &gfx.device,
                &gfx.queue,
                &view,
                out_format,
                size,
                out,
                cap_backdrop,
                self.frame_guide,
            );
        } else {
            gfx.renderer.render(&gfx.device, &gfx.queue, &view, &render_frame);
        }

        // #430 Tier 0: on-screen recorder feedback — the live "● REC" line, or the last
        // start failure. Drawn on the WINDOW *after* the readback above, so it is never
        // baked into the recorded file (that's Tier 1). This is the only feedback a
        // plugin-spawned visual gets: its stderr is discarded.
        if let Some((_, at)) = &record.error {
            if at.elapsed().as_secs_f32() >= 8.0 {
                record.error = None;
            }
        }
        if let Some((_, at)) = &record.note {
            if at.elapsed().as_secs_f32() >= 3.0 {
                record.note = None;
            }
        }
        let mut rec_lines: Vec<(String, [f32; 4])> = Vec::new();
        if let Some(h) = &record.hud {
            rec_lines.push((h.clone(), [1.0, 0.28, 0.24, 1.0]));
        }
        if let Some((msg, _)) = &record.error {
            rec_lines.push((msg.clone(), [1.0, 0.6, 0.25, 1.0]));
        }
        // The record-length toast (only while idle — the live "● REC" line already shows
        // the target during a take).
        if record.recorder.is_none() {
            if let Some((msg, _)) = &record.note {
                rec_lines.push((msg.clone(), [0.6, 0.85, 1.0, 1.0]));
            }
        }
        if !rec_lines.is_empty() {
            let px = (size.1 as f32 * 0.020).clamp(11.0, 26.0);
            gfx.overlay.draw_hud_panel(
                &gfx.device,
                &gfx.queue,
                &view,
                out_format,
                size,
                (0.0, 0.0, size.0 as f32, size.1 as f32),
                &rec_lines,
                2, // dock: top-right (clear of the #391 HUD's usual corner)
                px,
                [0.04, 0.02, 0.02, 0.55],
                0.3,
                (size.1 as f32 * 0.02).max(8.0),
                1.0,
            );
        }
        let cpu_dt = cpu_t0.elapsed().as_secs_f32() * 1000.0;
        self.cpu_ms += (cpu_dt - self.cpu_ms) * 0.1;
        // Closing timestamp (after all the render submits) + kick the async readback.
        if let Some(t) = gfx.gpu_timer.as_mut() {
            t.end(&gfx.device, &gfx.queue);
        }
        // Remember this frame's scene view-proj for the next frame's TAA reprojection
        // (#152 Tier 2). `render_frame` borrowed `uniforms`; it's dropped by here.
        self.prev_view_proj = unjittered_vp;

        // RT debug view (#195 Tier 0): a fullscreen ray query over the final frame
        // (drawn onto the swapchain, under the capture overlay). If its silhouettes
        // line up with the raster scene, the TLAS matches what was drawn.
        if rt_debug_on {
            if let Some(rtc) = gfx.rt.as_mut() {
                // Deliberately the UNJITTERED view-proj (#197 review): with TAA
                // on, the raster scene jitters sub-pixel per frame but the
                // presented image is the temporally resolved (jitter-free)
                // one — rays cast from the unjittered matrix compare against
                // exactly that. Matching the raw jitter would make the debug
                // overlay shimmer against the resolved scene instead.
                let inv_vp = Mat4::from_cols_array_2d(&unjittered_vp).inverse();
                let cam = Vec3::new(
                    uniforms.camera_pos[0],
                    uniforms.camera_pos[1],
                    uniforms.camera_pos[2],
                );
                // Ray reach: comfortably past the whole field from any orbit.
                // Clamped finite — a NaN/inf ray extent must never reach the
                // traversal hardware (see the GPU-fault note at the build).
                let mut t_max = (bounds.max - bounds.min).length().max(1.0) * 8.0;
                if !t_max.is_finite() {
                    t_max = 1e5;
                }
                let t_max = t_max.min(1e6);
                // Production-frame mode letterboxes the scene into a centred
                // fit rect; confine the debug rays to the same rect so their
                // aspect (and silhouettes) match the raster image (#197 review).
                let vp_rect = cap_out.map(|out| capture::letterbox_rect(size, out));
                rtc.draw_debug(
                    &gfx.device,
                    &gfx.queue,
                    &view,
                    out_format,
                    inv_vp,
                    cam,
                    s.rt[1] as u32,
                    t_max,
                    vp_rect,
                );
            }
        }

        // Capture overlay (#135 P2): composite the maths-account text on top, laid out
        // inside the production rect (or the full window for Native). After the blit,
        // before present — alpha-blended over the final image, EDR-safe.
        if self.overlay_on {
            let meta = overlay_meta::overlay_meta(generator);
            let ctx = overlay_meta::OverlayCtx {
                gen_phase: self.gen_phase as f32,
                gen_phase_hi: self.gen_phase,
                maxdip_phase: self.maxdip_phase as f32,
                angle: self.angle.x as f32,
                beat: self.beat_pos.fract() as f32,
                beat_pos: self.beat_pos,
                bpm: bpm as f32,
                s: &s,
            };
            let vals = (meta.eval)(&ctx);
            let frame_rect = match cap_out {
                Some(out) => capture::letterbox_rect(size, out),
                None => (0, 0, size.0, size.1),
            };
            let style = overlay::OverlayStyle {
                opacity: s.overlay[1],
                scale: s.overlay[2],
                show_title: s.overlay[3] != 0.0,
                show_desc: s.overlay[4] != 0.0,
                show_formula: s.overlay[5] != 0.0,
                show_readouts: s.overlay[6] != 0.0,
                show_handle: s.overlay[7] != 0.0,
                panel: [s.overlay[8], s.overlay[9], s.overlay[10], s.overlay[11]],
                text: [s.overlay[12], s.overlay[13], s.overlay[14]],
                handle: self.overlay_handle.clone(),
                title_override: self.overlay_title.clone(),
            };
            gfx.overlay.draw(
                &gfx.device,
                &gfx.queue,
                &view,
                out_format,
                size,
                frame_rect,
                &meta,
                &vals,
                &style,
            );

            // #380 Tier 2: the parameter-space inset — reproduce the source image's
            // (a,b) orbit plot. Draw the closed trajectory + the live current point,
            // normalized to the swept box [a0±Ra]×[b0±Rb]. Only for MapAttractor.
            if matches!(generator, GeneratorMode::MapAttractor) {
                let m = &s.mapattractor;
                let o = &s.maporbit;
                let playing = s.transport[0] > 0.5;
                let orbit = math::ParamOrbit {
                    mode: math::MapOrbitMode::from_u32(o[0] as u32),
                    a0: m[1],
                    b0: m[2],
                    ra: o[2],
                    rb: o[3],
                    fa: o[4],
                    fb: o[5],
                    psi: o[6],
                    a_drive: m[8],
                    b_drive: m[9],
                };
                // Box half-extents: the orbit radii (Lissajous), or a small pad for
                // Off/Linear so the single point sits sensibly near centre.
                let rx = orbit.ra.abs().max(0.5);
                let ry = orbit.rb.abs().max(0.5);
                let norm = |p: Vec2| -> (f32, f32) {
                    ((p.x - orbit.a0) / (2.0 * rx) + 0.5, (p.y - orbit.b0) / (2.0 * ry) + 0.5)
                };
                math::map_orbit_trajectory(&orbit, 160, &mut self.map_orbit_traj);
                let traj: Vec<(f32, f32)> = self.map_orbit_traj.iter().map(|&p| norm(p)).collect();
                let cur = norm(math::map_attractor_effective_ab(
                    m, o, self.beat_pos, self.gen_phase, playing,
                ));
                // A square inset in the frame's bottom-left corner.
                let (fx, fy, fw, fh) = (
                    frame_rect.0 as f32,
                    frame_rect.1 as f32,
                    frame_rect.2 as f32,
                    frame_rect.3 as f32,
                );
                let sz = (fw.min(fh) * 0.2).clamp(96.0, 320.0);
                let pad = fw.min(fh) * 0.03;
                let plot_rect = (fx + pad, fy + fh - sz - pad, sz, sz);
                gfx.overlay.draw_param_plot(
                    &gfx.device,
                    &gfx.queue,
                    &view,
                    out_format,
                    size,
                    plot_rect,
                    &traj,
                    cur,
                    style.opacity,
                );
            }

        }

        // #423 Tier 1 — the roofline inset: operational intensity (X, log FLOP/byte) vs
        // achievable performance (Y, log FLOP/s) against the MACHINE's own ceiling.
        // Shows which regime each scanned model is in (bandwidth slope vs compute
        // plateau) and how much of the machine it could possibly use. Drawn whenever the
        // atlas is active + the roofline toggle is on + a scan has happened —
        // deliberately NOT gated on the text overlay (`overlay_on`), because it has its
        // own checkbox and a toggle that silently does nothing until an unrelated one is
        // on is a trap. Follows the overlay opacity (default 0.9) for a consistent look.
        if s.atlas[1] > 0.5 && s.atlas[2] > 0.5 && !self.atlas_points.is_empty() {
            let rp = math::roofline_plot(&self.atlas_points, &self.atlas_profile);
            let rl_rect = match cap_out {
                Some(out) => capture::letterbox_rect(size, out),
                None => (0, 0, size.0, size.1),
            };
            let (fx, fy, fw, fh) =
                (rl_rect.0 as f32, rl_rect.1 as f32, rl_rect.2 as f32, rl_rect.3 as f32);
            let sz = (fw.min(fh) * 0.26).clamp(140.0, 420.0);
            let pad = fw.min(fh) * 0.03;
            // Bottom-right corner (the map plot takes bottom-left).
            let plot_rect = (fx + fw - sz - pad, fy + fh - sz - pad, sz, sz);
            gfx.overlay.draw_roofline(
                &gfx.device,
                &gfx.queue,
                &view,
                out_format,
                size,
                plot_rect,
                &rp,
                &self.atlas_profile.name,
                s.overlay[1],
            );
        }

        // Capture decoration (#135 P5): project the axis tips world→screen and label them
        // X/Y/Z via the overlay text pass, inside the production rect (same as the overlay).
        if axes_labels_on {
            let rect = match cap_out {
                Some(out) => capture::letterbox_rect(size, out),
                None => (0, 0, size.0, size.1),
            };
            let vp = Mat4::from_cols_array_2d(&uniforms.view_proj);
            // #423 Tier 1 — when the design-space constellation is what's loaded, the
            // frame's axes have MEANINGS, and an unlabelled frame is the one thing a
            // design space cannot afford: the whole point is which direction is memory
            // traffic. Name them (matching `math::design_space_constellation`'s layout:
            // X = bytes/token log, Y = operational intensity log, Z = bits/weight).
            let atlas_axes = self.atlas_is_loaded && s.atlas[1] > 0.5;
            let tips = if atlas_axes {
                [
                    (Vec3::new(axes_len, 0.0, 0.0), "bytes/tok", axes::AXIS_X),
                    (Vec3::new(0.0, axes_len, 0.0), "OI", axes::AXIS_Y),
                    (Vec3::new(0.0, 0.0, axes_len), "bits/w", axes::AXIS_Z),
                ]
            } else {
                [
                    (Vec3::new(axes_len, 0.0, 0.0), "X", axes::AXIS_X),
                    (Vec3::new(0.0, axes_len, 0.0), "Y", axes::AXIS_Y),
                    (Vec3::new(0.0, 0.0, axes_len), "Z", axes::AXIS_Z),
                ]
            };
            let mut markers: Vec<(f32, f32, &str, [f32; 4])> = Vec::new();
            for (tip, lbl, col) in tips {
                let clip = vp * tip.extend(1.0);
                if clip.w <= 0.0 {
                    continue; // behind the camera
                }
                let ndc = clip.truncate() / clip.w;
                let px = rect.0 as f32 + (ndc.x * 0.5 + 0.5) * rect.2 as f32;
                let py = rect.1 as f32 + (0.5 - ndc.y * 0.5) * rect.3 as f32;
                markers.push((px, py, lbl, [col[0], col[1], col[2], 1.0]));
            }
            let label_px = (rect.3 as f32 * 0.03).max(10.0);
            gfx.overlay.draw_markers(
                &gfx.device,
                &gfx.queue,
                &view,
                out_format,
                size,
                &markers,
                label_px,
            );
        }

        // Calibrated meter HUD (#333 Tiers 1–2): a numeric LUFS / dBTP / correlation
        // readout drawn top-left, gated by the `meter_hud` param (audiometer[10]).
        if s.audiometer[10] > 0.5 {
            let rect = match cap_out {
                Some(out) => capture::letterbox_rect(size, out),
                None => (0, 0, size.0, size.1),
            };
            let am = &s.audiometer;
            let db = |v: f32| if v <= -119.0 { "−∞".to_string() } else { format!("{v:.1}") };
            let white = [0.92, 0.94, 1.0, 0.95];
            let tp_col = if am[4] > -1.0 { [1.0, 0.35, 0.3, 0.98] } else { white };
            let lines = vec![
                (format!("LUFS  M {}  S {}  I {}", db(am[0]), db(am[1]), db(am[2])), white),
                (
                    format!("dBTP {}   LRA {:.1}   corr {:+.2}", db(am[4]), am[3].max(0.0), am[5]),
                    tp_col,
                ),
            ];
            let px = (rect.3 as f32 * 0.024).max(11.0);
            let x = rect.0 as f32 + rect.2 as f32 * 0.02;
            let y = rect.1 as f32 + rect.3 as f32 * 0.03;
            gfx.overlay
                .draw_hud(&gfx.device, &gfx.queue, &view, out_format, size, &lines, x, y, px);
        }

        // Calibrated INSTRUMENT HUD (#333 Tier 3): delivery-target over/under + the
        // true-peak / phase alarms — the "read over/under target at a glance" panel.
        // Gated by the `an_reference_hud` param (analytical[5]).
        if s.analytical[5] > 0.5 {
            let rect = match cap_out {
                Some(out) => capture::letterbox_rect(size, out),
                None => (0, 0, size.0, size.1),
            };
            let am = &s.audiometer;
            let an = &s.analytical;
            let target = an[1];
            let integrated = am[2];
            let db = |v: f32| if v <= -119.0 { "−∞".to_string() } else { format!("{v:.1}") };
            let green = [0.55, 0.95, 0.6, 0.96];
            let amber = [1.0, 0.78, 0.3, 0.97];
            let red = [1.0, 0.35, 0.3, 0.98];
            let calibrated = an[0] > 0.5;
            let mut lines: Vec<(String, [f32; 4])> = Vec::new();
            lines.push((
                format!("INSTRUMENT · {}", if calibrated { "CALIBRATED" } else { "expressive" }),
                if calibrated { green } else { amber },
            ));
            // Delivery target: over / under, coloured by whether we're within ±1 LU.
            if integrated > -119.0 {
                let err = math::loudness_target_error(integrated, target);
                let (word, col) = if err.abs() <= 1.0 {
                    ("on target", green)
                } else if err > 0.0 {
                    ("OVER", red)
                } else {
                    ("under", amber)
                };
                lines.push((
                    format!("target {} → I {}  ({:+.1} LU {})", db(target), db(integrated), err, word),
                    col,
                ));
            } else {
                lines.push((format!("target {} LUFS  (integrating…)", db(target)), amber));
            }
            // Alarms: true-peak over the ceiling, and phase / correlation.
            let tp_alarm = math::true_peak_alarm(am[4], an[3], 1.0);
            if tp_alarm > 0.0 {
                lines.push((format!("⚠ TRUE-PEAK OVER  {} dBTP (ceil {})", db(am[4]), db(an[3])), red));
            }
            if math::correlation_alarm(am[5], an[4]) > 0.0 {
                lines.push((format!("⚠ PHASE  corr {:+.2} (thr {:+.2})", am[5], an[4]), red));
            }
            let px = (rect.3 as f32 * 0.024).max(11.0);
            let x = rect.0 as f32 + rect.2 as f32 * 0.02;
            // Sit below the basic meter HUD if it's showing, else at the top.
            let below = if s.audiometer[10] > 0.5 { 3.4 } else { 0.0 };
            let y = rect.1 as f32 + rect.3 as f32 * 0.03 + below * px * 1.35;
            gfx.overlay
                .draw_hud(&gfx.device, &gfx.queue, &view, out_format, size, &lines, x, y, px);
        }

        // Quantitative instrumentation HUD (#391 Tier 1): probe / energy-ledger /
        // Poynting-flux read-outs, sampled from the SAME field kernels drawn on screen.
        // Gated by `instr_hud` (instrument[0]); only the field generators produce a field.
        {
            let inst = &s.instrument;
            let want_hud = inst[0] > 0.5;
            let want_csv = inst[15] > 0.5;
            if want_hud || want_csv {
                if let Some(field) =
                    instrument_field(&s, self.gen_phase as f32, self.maxdip_phase as f32, self.beat_pos)
                {
                    let probe_p = Vec3::new(inst[2], inst[3], inst[4]);
                    // When the FDTD solver is driving the Volume cloud, read the LIVE
                    // grid (not the closed form) so the probe/ledger/flux HUD + CSV
                    // describe the SAME field the picture shows (the Tier-1 contract).
                    let fdtd_active = generator == GeneratorMode::MaxwellField
                        && volume
                        && s.fdtd[0] > 0.5
                        && self.fdtd_sim.fdtd.is_some();
                    let probe = if fdtd_active {
                        self.fdtd_sim.fdtd.as_ref().unwrap().probe(probe_p)
                    } else {
                        field.probe(probe_p)
                    };
                    let t = self.gen_phase as f32;

                    // CSV logging: truncate + header on the rising edge, append each frame.
                    if want_csv {
                        use std::io::Write as _;
                        let path = ipc::probe_csv_path();
                        if !self.instr_csv_active {
                            // Only latch active once the file is actually (re)created with
                            // its header — a failed open stays inactive so the next frame
                            // retries the truncate + header rather than blind-appending.
                            if let Ok(mut f) = std::fs::File::create(&path) {
                                let _ = writeln!(f, "{}", math::probe_csv_header());
                                let _ = writeln!(f, "{}", math::probe_csv_row(t, probe_p, &probe));
                                self.instr_csv_active = true;
                            }
                        } else if let Ok(mut f) =
                            std::fs::OpenOptions::new().create(true).append(true).open(&path)
                        {
                            let _ = writeln!(f, "{}", math::probe_csv_row(t, probe_p, &probe));
                        }
                    } else {
                        self.instr_csv_active = false;
                    }

                    if want_hud {
                        let rect = match cap_out {
                            Some(out) => capture::letterbox_rect(size, out),
                            None => (0, 0, size.0, size.1),
                        };
                        let cyan = [0.55, 0.9, 1.0, 0.96];
                        let warm = [1.0, 0.8, 0.45, 0.96];
                        let dim = [0.75, 0.8, 0.9, 0.92];
                        let mut lines: Vec<(String, [f32; 4])> = Vec::new();
                        let em = probe.kind == math::ProbeKind::Em;
                        lines.push((
                            format!("INSTRUMENT · {}", if em { "E/B field" } else { "acoustic field" }),
                            cyan,
                        ));
                        if inst[1] > 0.5 {
                            if em {
                                lines.push((
                                    format!(
                                        "probe ({:+.1},{:+.1},{:+.1})  |E| {:.3} V/m  |B| {:.3} T",
                                        probe_p.x, probe_p.y, probe_p.z,
                                        probe.primary.length(), probe.secondary.length(),
                                    ),
                                    dim,
                                ));
                            } else {
                                lines.push((
                                    format!(
                                        "probe ({:+.1},{:+.1},{:+.1})  p {:+.3} Pa  |u| {:.3} m/s",
                                        probe_p.x, probe_p.y, probe_p.z, probe.scalar, probe.secondary.length(),
                                    ),
                                    dim,
                                ));
                            }
                            lines.push((
                                format!("|S| {:.4} W/m²  (flux dir {:+.2},{:+.2},{:+.2})",
                                    probe.flux.length(),
                                    probe.flux.normalize_or_zero().x,
                                    probe.flux.normalize_or_zero().y,
                                    probe.flux.normalize_or_zero().z,
                                ),
                                dim,
                            ));
                        }
                        if inst[5] > 0.5 {
                            let led = if fdtd_active {
                                self.fdtd_sim.fdtd.as_ref().unwrap().energy_ledger(
                                    Vec3::ZERO, inst[6].max(0.25), inst[7] as u32,
                                )
                            } else {
                                math::field_energy_ledger(
                                    &field, Vec3::ZERO, inst[6].max(0.25), inst[7] as u32,
                                )
                            };
                            let (a_lbl, b_lbl) = if em { ("E", "B") } else { ("compression", "kinetic") };
                            lines.push((
                                format!(
                                    "ledger  {} {:.3}  {} {:.3}  total {:.3}  ({:.0}% {})",
                                    a_lbl, led.u_a, b_lbl, led.u_b, led.total, led.balance * 100.0, a_lbl,
                                ),
                                warm,
                            ));
                            lines.push((
                                format!("net radiated flux {:+.4}  ({} samples)", led.net_flux, led.samples),
                                dim,
                            ));
                        }
                        if inst[8] > 0.5 {
                            let center = Vec3::new(inst[9], inst[10], inst[11]);
                            let axis = organon_core::params::FluxAxis::from_u32(inst[13] as u32);
                            let flux = if fdtd_active {
                                self.fdtd_sim.fdtd.as_ref().unwrap().poynting_flux_through_plane(
                                    center, axis.normal(center), inst[12].max(0.05), inst[14] as u32,
                                )
                            } else {
                                math::poynting_flux_through_plane(
                                    &field, center, axis.normal(center), inst[12].max(0.05), inst[14] as u32,
                                )
                            };
                            lines.push((
                                format!("flux patch @ ({:+.1},{:+.1},{:+.1})  ∮S·n̂ = {:+.4} W", center.x, center.y, center.z, flux),
                                cyan,
                            ));
                        }
                        if want_csv {
                            lines.push(("● logging probe CSV".to_string(), warm));
                        }
                        // Presentation (instrument2): a rounded backing panel for
                        // contrast, an overall size dial, and a dock corner.
                        let i2 = &s.instrument2;
                        let scale = i2[2].clamp(0.4, 3.0);
                        let px = (rect.3 as f32 * 0.024).max(11.0) * scale;
                        let dock = i2[3] as u32;
                        // Only stack clear of the #333 meter HUDs when docked top-left
                        // (that's where they live); other corners are free.
                        let stack = if dock == 0 {
                            let mut b = 0.0f32;
                            if s.audiometer[10] > 0.5 { b += 3.4; }
                            if s.analytical[5] > 0.5 { b += 5.0; }
                            b
                        } else {
                            0.0
                        };
                        let bg = [0.02, 0.03, 0.05, i2[0].clamp(0.0, 1.0)];
                        let margin = (rect.3 as f32 * 0.03).max(6.0);
                        let area =
                            (rect.0 as f32, rect.1 as f32, rect.2 as f32, rect.3 as f32);
                        gfx.overlay.draw_hud_panel(
                            &gfx.device, &gfx.queue, &view, out_format, size, area,
                            &lines, dock, px, bg, i2[1].clamp(0.0, 1.0), margin, stack,
                        );
                    }
                } else {
                    // Non-field generator: nothing to measure this frame.
                    self.instr_csv_active = false;
                }
            } else {
                self.instr_csv_active = false;
            }
        }
        // #554 Tier 4 — the UI layer, last of all: after the composite, after the capture
        // overlay, before present.
        //
        // Last is the whole point. The composite owns exposure → bloom → tone-map; drawing the
        // interface before it would tone-map the theme's greys, bloom its bright text into the
        // scene, and dim the controls whenever the EV changed. Drawing after it means the UI is
        // exactly the colour it asked to be.
        //
        // **Window targets only.** An offscreen frame is a picture of the *scene* — the frame
        // mirror and the production recorder both want the world without an interface painted
        // over it.
        // ⚠️ **Borrow the layer, never move it out.** This block used to read
        // `(gfx.ui.take(), ui_geometry(..))` as one tuple pattern, and a tuple's elements are
        // evaluated left to right *before* the pattern is matched — so every offscreen frame
        // took the layer out of `gfx.ui`, failed to match on the `None` geometry, and skipped
        // the `gfx.ui = Some(ui)` that would have put it back. One frame mirror or recorder
        // frame destroyed the interface for the life of the process: no HUD, and **U** silently
        // dead, because its handler reaches for the same `gfx.ui` and finds `None`.
        //
        // `as_mut` cannot lose it. `gfx.ui` and `gfx.device`/`gfx.queue` are disjoint fields, as
        // are `self.gfx` and the `self.frame_ms`/`hdr_*` the closure reads, so nothing here
        // needed ownership in the first place.
        if let Some(geometry) = ui_geometry(&out, target.ui_scale_factor) {
            if let Some(ui) = gfx.ui.as_mut() {
                // The swapchain format moves under us when HDR is toggled, and the egui
                // pipeline is bound to its target format.
                ui.set_format(&gfx.device, out.format);
                // `_deferred` is `()` here: the winit backend holds its window and does its own
                // cursor/clipboard work. A backend that cannot — route C's — returns its plan
                // for the host to apply, which is why `paint` reports rather than swallows.
                let _deferred = ui.paint(
                    &gfx.device,
                    &gfx.queue,
                    &view,
                    geometry,
                    |ctx| {
                        ui_layer::hud(
                            ctx,
                            ui_layer::HudState {
                                generator: format!("{generator:?}"),
                                frame_ms: self.frame_ms,
                                size: out.size,
                                hdr_enabled: self.hdr.hdr_enabled,
                                hdr_max: self.hdr.hdr_max,
                            },
                        )
                    },
                );
            }
        }

        // (#572 stage 3) Present belongs to whoever acquired the image, and happens after this
        // call returns. The frame is done the moment its passes are submitted; the caller owns
        // the texture and decides what happens to it next — present it, read it back, or hand it
        // to egui as a pane.

        // Report the live render resolution: the window title (always visible while
        // windowed) + the feedback channel the editor reads (works fullscreen too).
        // In capture mode the scene renders at the production size, not the window.
        let rdims = render::scaled_render_size(render_size, render_scale);
        if rdims != self.last_render_dims {
            self.last_render_dims = rdims;
            let pct = (render_scale * 100.0).round() as i32;
            // Reported, not performed (#572 stage 3) — an offscreen host has no title bar and
            // simply drops it. The `Feedback` write below is the channel that works either way.
            requests.title = Some(format!(
                "Organon — render {}×{} ({}%)",
                rdims.0, rdims.1, pct
            ));
        }
        // Mirror the GPU frame time for the agent's read_feedback tool (finding #5).
        self.performer.gpu_ms = gfx.gpu_timer.as_ref().map(|t| t.ms()).unwrap_or(0.0);
        if let Some(fb) = self.feedback.as_mut() {
            fb.write(ipc::Feedback {
                seq: 0,
                layout_version: 0,
                scale: render_scale,
                width: rdims.0,
                height: rdims.1,
                fps: 1000.0 / self.frame_ms.max(0.1),
                // Production-frame size (0,0 = Native = the window).
                out_w: cap_out.map(|o| o.0).unwrap_or(0),
                out_h: cap_out.map(|o| o.1).unwrap_or(0),
                // Hardware RT (#195 Tier 0): the editor greys the card out
                // when unavailable and shows the live TLAS rebuild cost.
                rt_available: gfx.rt.is_some() as u32,
                tlas_ms: rt_tlas_ms,
                // Neural acceleration (#200 Tier 2): adapter support detection.
                coopmat_available: gfx.coopmat_available as u32,
                f16_available: gfx.f16_available as u32,
                // Metal interop island (#200 Tier 3): startup probe result.
                metal_island_available: gfx.island.available as u32,
                tensor_gflops: gfx.island.tensor_gflops,
                // Path tracer (#200 Tier 4): ground-truth active + accumulated spp.
                // organon#217 T5: the live state, so the editor's "path tracer: ON — N spp"
                // line reads true during a glyph dwell too; with no ring it is the toggle.
                pathtrace_active: (pt_active && gfx.rt.is_some()) as u32,
                pathtrace_spp: self.pathtrace_spp,
                // Workload telemetry (#277 Tier 2): drawn instance count + the
                // smoothed CPU encode cost, for the status bar's headroom meters.
                instances: self.geom.instances.len() as u32,
                cpu_ms: self.cpu_ms,
                // GPU timing (#277 Tier 3): true GPU frame ms + whether the device
                // supports it (0 / false → the editor shows "n/a").
                gpu_ms: gfx.gpu_timer.as_ref().map(|t| t.ms()).unwrap_or(0.0),
                gpu_timing_available: gfx.gpu_timer.is_some() as u32,
                // Neural radiance cache (#256 Tier 0): live training loss + state.
                nrc_loss: self.nrc_loss,
                nrc_state: self.nrc_state,
            });
        }
    }

    /// #430 Tier 0: finalize an in-progress take before the process goes away.
    ///
    /// Closing the window / pressing Escape just exits the event loop, which would drop
    /// ffmpeg's child mid-stream: the readback ring never drains, stdin never EOFs, and the
    /// encode worker is never joined — leaving a truncated, usually unplayable file. A take
    /// is expensive to re-shoot, so always land it properly on the way out.
    fn finalize_recording(&mut self) {
        self.record.chunk_armed = false;
        if let Some(rec) = self.record.recorder.take() {
            // A gated clip never opened its shutter, so its file is empty — drop it rather
            // than leave a zero-frame stub behind.
            let gated = rec.is_gated();
            match self.gfx.as_ref() {
                // The normal path: flush in-flight maps, drain the worker, wait on ffmpeg.
                Some(gfx) => {
                    if gated {
                        self.record.pending_finalizers.push(rec.discard(&gfx.device));
                    } else {
                        rec.finish(&gfx.device);
                    }
                }
                // No device to poll with (we never got one): dropping the Recorder still
                // closes the channel, so the worker drains and EOFs ffmpeg — we just can't
                // wait on it.
                None => drop(rec),
            }
        }
        // #430 chunk mode: clips already rolled are being muxed on background threads. Join
        // them, or exiting here kills the process mid-mux and strands `.videotmp` files —
        // and in a chunk session that could be most of the take.
        for h in self.record.pending_finalizers.drain(..) {
            let _ = h.join();
        }
        self.record.hud = None;
    }

    /// The world's half of a window resize (#572 stage 3).
    ///
    /// Reconfiguring the swapchain and re-asserting EDR afterwards are the *host's* — they are
    /// surface and layer operations. Nothing here needs doing on a resize any more: the frame
    /// reads its size from the target every time, so a changed window simply arrives as a
    /// differently-sized target. Kept as an explicit no-op with this note rather than deleted,
    /// because "resize does nothing in the world" is a fact worth stating once instead of
    /// rediscovering.
    pub fn on_resized(&mut self) {}

    /// (Re)assert the metal layer's EDR state + colorspace for the current
    /// `hdr_enabled` / `hdr_wide`, and refresh the measured headroom. Cheap enough
    /// to call after any reconfigure or on a colorspace-only change.
    /// Whether true-HDR output is wanted, and in which gamut — the *intent*, which the world
    /// owns because the **H** key and the editor's checkbox set it. Whether it can be granted is
    /// the host's answer, delivered as `FrameTarget::hdr_max` (#572 stage 3).
    pub fn hdr_request(&self) -> (bool, bool) {
        (self.hdr.hdr_enabled, self.hdr.hdr_wide)
    }
}

impl World {
    /// AI Performer (#317 Tier 1) per-frame step. Edge-detects the plugin-published
    /// `Shared.agent[8]` counters (chat / plan / release), drives the agent worker
    /// thread + debug plan executor, then applies the override lane to `s`. All inert
    /// when the block is zero (standalone visual / plugin without the Mind card engaged).
    /// #648 T3 — an associated fn, not a method: it mutates FOUR clusters, so no single
    /// one owns it. Naming them in the signature is the point — the compiler now knows
    /// exactly which subsystems this touches, and `frame_body` can hold `&mut` locals of
    /// every other cluster across the call.
    fn step_agent(
        perf: &mut PerformerLink,
        cmd: &mut CmdChannel,
        rec: &mut RecordState,
        geom: &mut Geometry,
        frame_ms: f32,
        cpu_ms: f32,
        s: &mut ipc::Shared,
    ) {
        // Seed the edge-detect baselines from the FIRST Shared read (finding #6). The
        // plugin may already expose non-zero counters (it kept running while only the
        // visual restarted); seeding from the first snapshot means only genuinely NEW
        // bumps fire, so a restart doesn't replay the last chat / plan / release. The
        // chat cursor is seeded to the sidecar's current line count for the same reason.
        if !perf.baseline_seeded {
            perf.baseline_seeded = true;
            perf.last_chat_gen = s.agent[1] as u32;
            perf.last_plan_gen = s.agent[2] as u32;
            perf.last_release_gen = s.agent[3] as u32;
            perf.last_name_gen = s.agent[4] as u32;
            perf.chat_lines_consumed = std::fs::read_to_string(ipc::chat_sidecar_path())
                .map(|b| b.lines().filter(|l| !l.trim().is_empty()).count())
                .unwrap_or(0);
            // #425: an in-flight save when the visual restarts adopts its `name_gen` above,
            // so no edge fires for it. Service any request the (still-running) editor left
            // pending, once, on startup. `service_name_request` deletes the request file
            // after reading, so this only ever re-runs a genuinely-unserviced name.
            service_name_request();
        }

        // Publish a live state/feedback snapshot for read_state / read_feedback (finding
        // #5): the state half from `Shared`, the perf half from the render loop.
        {
            let fps = 1000.0 / frame_ms.max(0.1);
            let snap = agent::LiveState::from_shared(s).with_perf(
                fps,
                perf.gpu_ms,
                cpu_ms,
                geom.instances.len() as u32,
            );
            if let Ok(mut st) = perf.state.lock() {
                *st = snap;
            }
        }

        // "Release agent": clear all holds + selector overrides.
        let release_gen = s.agent[3] as u32;
        if release_gen != perf.last_release_gen {
            perf.last_release_gen = release_gen;
            if let Ok(mut lane) = perf.lane.lock() {
                lane.release_all();
            }
            mind_log::append(mind_log::MindEvent::Note, "agent", "released all holds");
        }

        // Debug executor path: a hand-written phrase-plan JSON — a scriptable phrase
        // sequencer that needs no model at all. Parse it and dispatch immediately (the
        // full LFO/bar-latched executor is Round 2; `rails_latch_step` is the seam).
        let plan_gen = s.agent[2] as u32;
        if plan_gen != perf.last_plan_gen {
            perf.last_plan_gen = plan_gen;
            if let Ok(body) = std::fs::read_to_string(ipc::plan_sidecar_path()) {
                if let Some(plan) = agent::PhrasePlan::parse(&body) {
                    mind_log::append(mind_log::MindEvent::Plan, "editor", &body);
                    let act = plan.as_action();
                    if let Ok(mut lane) = perf.lane.lock() {
                        // #317 UI-sync: mirror onto the editor sliders — INSIDE the lock, so the
                        // apply channel and the override lane never diverge (if the lock is
                        // unavailable, neither the lane nor the channel records the action).
                        append_agent_apply(&act);
                        for out in agent::dispatch(&mut lane, act) {
                            let ev = if out.is_applied() {
                                mind_log::MindEvent::Action
                            } else {
                                mind_log::MindEvent::Reject
                            };
                            mind_log::append(ev, "plan", out.summary());
                        }
                    }
                }
            }
        }

        // #452 Tier 2: the CLI command channel (`organon set/do/release/gen/surf/mat`).
        // External local agents (Bianca) APPEND CliOp lines to the sidecar; there is no
        // `Shared` gen counter (the CLI is deliberately never an IPC writer), so growth
        // is self-detected by file length — one `stat` per frame when idle. Same
        // append-and-drain cursor discipline as the editor apply channel; the seed
        // happens at CONSTRUCTION (see the field init), so commands issued while the
        // visual was down never replay, while ones appended right after launch still
        // drain. Each op flows through
        // the SAME dispatch + override lane as the Performer, so last-touched-wins,
        // slider mirroring (apply channel), and the mind-log corpus come for free.
        let cli_len_now = std::fs::metadata(ipc::cli_cmd_path())
            .map(|m| m.len())
            .unwrap_or(0);
        // A failed read yields `None` → `cli_drain_step` returns nothing and the
        // cached state stays untouched, so the next frame simply retries (a
        // committed cursor over a failed read could drop or replay ops).
        let cli_body = if cli_len_now != cmd.cli_len {
            if cli_len_now == 0 {
                Some(String::new())
            } else {
                std::fs::read_to_string(ipc::cli_cmd_path()).ok()
            }
        } else {
            None
        };
        if let Some((cli_lines, new_len, new_cursor)) =
            agent::cli_drain_step(cmd.cli_len, cli_len_now, cli_body.as_deref(), cmd.cli_cursor)
        {
            cmd.cli_len = new_len;
            cmd.cli_cursor = new_cursor;
            for line in &cli_lines {
                let Some(op) = agent::CliOp::parse(line) else {
                    mind_log::append(
                        mind_log::MindEvent::Reject,
                        "cli",
                        &format!("unparseable op: {line}"),
                    );
                    continue;
                };
                match op {
                    agent::CliOp::Release(None) => {
                        if let Ok(mut lane) = perf.lane.lock() {
                            lane.release_all();
                            // Tell the editor to stop mirroring (values stay put) —
                            // the same `release` line "Release agent" produces.
                            append_agent_apply_line("release");
                        }
                        mind_log::append(mind_log::MindEvent::Note, "cli", "released all holds");
                    }
                    agent::CliOp::Release(Some(id)) => {
                        if let Ok(mut lane) = perf.lane.lock() {
                            lane.release_one(&id);
                        }
                        // No editor op needed: the mirrored value stays where the
                        // hold left it; the editor only re-applies NEW ops.
                        mind_log::append(
                            mind_log::MindEvent::Note,
                            "cli",
                            &format!("released {id}"),
                        );
                    }
                    op => match op.into_action() {
                        Some(act) => {
                            if let Ok(mut lane) = perf.lane.lock() {
                                // Mirror inside the lock (the #317 UI-sync rule): the
                                // apply channel and the lane never diverge.
                                append_agent_apply(&act);
                                for out in agent::dispatch(&mut lane, act) {
                                    let ev = if out.is_applied() {
                                        mind_log::MindEvent::Action
                                    } else {
                                        mind_log::MindEvent::Reject
                                    };
                                    mind_log::append(ev, "cli", out.summary());
                                }
                            }
                        }
                        None => {
                            mind_log::append(
                                mind_log::MindEvent::Reject,
                                "cli",
                                &format!("bad plan json: {line}"),
                            );
                        }
                    },
                }
            }
        }

        // #452 Tier 3 ("the eyes"): the snap/record request channel. Same file-length +
        // append-and-drain discipline as the CLI command channel above (seeded at
        // construction). Each request is `<nonce> <verb>`; we act and append the outcome
        // to the reply channel keyed by that nonce. `snap` defers to render() (it needs
        // the production texture); `record start/stop` set the recorder's pending toggle
        // and reply from the record handler (only there is the file path known).
        let eyes_len_now = std::fs::metadata(ipc::eyes_cmd_path())
            .map(|m| m.len())
            .unwrap_or(0);
        let eyes_body = if eyes_len_now != cmd.eyes_len {
            if eyes_len_now == 0 {
                Some(String::new())
            } else {
                std::fs::read_to_string(ipc::eyes_cmd_path()).ok()
            }
        } else {
            None
        };
        if let Some((eyes_lines, new_len, new_cursor)) =
            agent::cli_drain_step(cmd.eyes_len, eyes_len_now, eyes_body.as_deref(), cmd.eyes_cursor)
        {
            cmd.eyes_len = new_len;
            cmd.eyes_cursor = new_cursor;
            for line in &eyes_lines {
                let Some((nonce, req)) = organon_core::eyes::EyesReq::parse(line) else {
                    mind_log::append(
                        mind_log::MindEvent::Reject,
                        "cli",
                        &format!("unparseable eyes request: {line}"),
                    );
                    continue;
                };
                use organon_core::eyes::EyesReq;
                match req {
                    EyesReq::Snap { path } => {
                        // Deferred to render() — the production texture is ensured there.
                        // A second snap before the first renders just supersedes it.
                        cmd.snap_pending = Some((nonce, std::path::PathBuf::from(path)));
                    }
                    EyesReq::RecordStart { bars } => {
                        if rec.recorder.is_some() {
                            append_eyes_reply(&nonce, &Err("already recording".into()));
                        } else {
                            rec.bars = bars;
                            rec.perfect_pending = false;
                            rec.toggle_pending = true;
                            cmd.eyes_record_pending = Some((nonce, true));
                        }
                    }
                    EyesReq::RecordStop => {
                        if rec.recorder.is_none() {
                            append_eyes_reply(&nonce, &Err("not recording".into()));
                        } else {
                            rec.toggle_pending = true;
                            cmd.eyes_record_pending = Some((nonce, false));
                        }
                    }
                }
                mind_log::append(mind_log::MindEvent::Note, "cli", &format!("eyes: {line}"));
            }
        }

        // #425 intelligent preset names: a `name_gen` bump means the editor saved a preset
        // with auto-naming on and wrote the scene identity to the request sidecar. Service it
        // OFF the render thread (a detached worker owns the blocking HTTP) into a per-id reply
        // file the editor drains. Keyed off a configured endpoint only — no `agent_on`, no
        // worker/lane involvement.
        let name_gen = s.agent[4] as u32;
        if name_gen != perf.last_name_gen {
            perf.last_name_gen = name_gen;
            service_name_request();
        }

        // Chat: append-and-drain (finding #3). The editor APPENDS each Send as its own
        // line to the chat sidecar and bumps `chat_gen`; on a change we drain EVERY line
        // appended since the cursor (not just the latest), so rapid sends before the
        // visual consumes the counter are never dropped. The worker is spawned lazily.
        let chat_gen = s.agent[1] as u32;
        if chat_gen != perf.last_chat_gen {
            perf.last_chat_gen = chat_gen;
            if let Ok(body) = std::fs::read_to_string(ipc::chat_sidecar_path()) {
                let (pending, cursor) = agent_chat_drain(&body, perf.chat_lines_consumed);
                perf.chat_lines_consumed = cursor;
                if !pending.is_empty() {
                    perf.ensure_agent_worker();
                    if let Some(tx) = &perf.tx {
                        for msg in pending {
                            let _ = tx.send(msg);
                        }
                    }
                }
            }
        }

        // Apply the override lane (last-touched-wins) onto the working snapshot.
        if let Ok(mut lane) = perf.lane.lock() {
            lane.apply(s);
            // Publish the held-param readout + last reply for the editor's Mind card.
            let held = lane.held_ids().join(",");
            let reply = perf.reply.lock().map(|r| r.clone()).unwrap_or_default();
            let status = format!("{held}\n{reply}\n");
            if status != perf.status_written {
                let _ = std::fs::write(ipc::agent_status_path(), &status);
                perf.status_written = status;
            }
        }
    }
}


/// What the host must do once [`World::on_window_event`] has had the event (#572 stage 2).
///
/// The world handles the event itself — camera, keys, redraw, the UI layer's pointer routing.
/// The single thing it cannot do is end the event loop, because winit's `ActiveEventLoop`
/// belongs to whoever called `run_app`. So quitting comes back as a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventResponse {
    /// Keep running.
    Continue,
    /// Draw a frame. The world can no longer do this itself — acquiring and presenting a
    /// swapchain image is the host's, so `RedrawRequested` becomes a request rather than a call
    /// (#572 stage 3).
    Redraw,
    /// The user asked to quit (window close, or **Esc**). Any in-progress recording has
    /// already been landed — the host only has to exit.
    Exit,
}

/// The window seam (#572 stages 2–3 — the orphan-rule boundary, and the surface leaving).
///
/// `impl ApplicationHandler for World` cannot be written here (winit's trait, our type, and the
/// binary is a different crate root), so these are what `bin/visual.rs`'s `VisualApp` wrapper
/// forwards to: [`is_attached`](World::is_attached) and [`attach_gpu`](World::attach_gpu) to
/// bring up the device, [`on_window_event`](World::on_window_event) for input, and
/// [`render_into`](World::render_into) + [`present`](World::present) for each frame.
///
/// **Stage 3 finished the job this comment used to promise.** The world owns no window, no
/// surface and no swapchain: the host acquires an image, states its properties on a
/// [`FrameTarget`], and applies the [`FrameRequests`] that come back. What it cannot do — end
/// the event loop, or draw — comes back as an [`EventResponse`].
impl World {
    /// Whether the graphics stack has been built yet. `resumed` can fire more than once, and
    /// building a second device would leak the first.
    pub fn is_attached(&self) -> bool {
        self.gfx.is_some()
    }

    /// Adopt a wgpu device the host created, and build everything that draws on it
    /// (#572 stage 3 — this was the tail of `attach_window`).
    ///
    /// The host owns the half that came before: the instance, the **surface**, the adapter, the
    /// feature/limit negotiation, and the swapchain configuration. It has to — a surface needs a
    /// window handle, and route C's editor gets an `NSView` rather than a winit window. What the
    /// world keeps is everything downstream of a `Device`, which is identical for every host.
    ///
    /// `format` is the host's current output format. `coopmat_available` / `f16_available` are
    /// adapter facts the host already queried, passed through because the editor reports them.
    ///
    /// **`ui` is built by the host, not here** (#593 Tier 3). This used to take an
    /// `Option<&winit::Window>` and construct the layer itself, which is precisely the winit
    /// coupling Tier 3 removes: only the host knows which platform backend it has —
    /// `winit_platform::ui_layer(&device, window, format)` for the visual, a baseview one for
    /// route C's editor. `None` = this world draws no interface, which is what an offscreen
    /// consumer wants.
    pub fn attach_gpu(
        &mut self,
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        ui: Option<ui_layer::UiLayer>,
        coopmat_available: bool,
        f16_available: bool,
    ) {
        if self.gfx.is_some() {
            return;
        }
        // GPU frame timer (#277 Tier 3): only built when the device actually got
        // TIMESTAMP_QUERY (the intersection above may drop it on a spare adapter).
        let gpu_timer = gpu_timer::GpuTimer::new(&device, &queue);
        let renderer = render::Renderer::new(&device, &queue, format);
        let capture = capture::Capture::new(&device, format);
        let overlay = overlay::Overlay::new(&device, &queue, format);
        let rt = rt::RtContext::new(&device);
        // Metal interop island (#200 Tier 3): probe once at startup (off the render
        // loop — the probe's blocking wait is fine here, never per-frame).
        let island = metal_island::probe(&device);
        if island.available {
            eprintln!(
                "[island] Metal interop available on '{}' ({} GFLOPs tensor)",
                island.device_name, island.tensor_gflops
            );
        }
        self.gfx = Some(Gfx {
            device,
            queue,
            renderer,
            // The composite/FX/temporal pipelines were just built for the host's current
            // format; the per-frame check rebuilds them if a target ever asks for a different
            // one — which is exactly what an HDR swap now looks like from in here.
            out_format: format,
            capture,
            overlay,
            rt,
            coopmat_available,
            f16_available,
            island,
            gpu_timer,
            ui,
        });
    }

    /// Handle one window event. The body is `bin/visual.rs`'s old `window_event` unchanged
    /// apart from the two `event_loop.exit()` sites, which now set the returned
    /// [`EventResponse`].
    ///
    /// **This is the winit host's entry point and stays winit-typed**, deliberately: the
    /// visual's whole keymap (**H**, **U**, **Esc**, **R**, …) lives in the body below, and a
    /// baseview host never calls it. What #593 Tier 3 removed is the `&winit::Window` that used
    /// to ride alongside — the UI layer needs the window's *geometry*, not the window, so the
    /// host states it.
    pub fn on_window_event(
        &mut self,
        geometry: WindowGeometry,
        event: WindowEvent,
    ) -> EventResponse {
        // #554 Tier 4 — the UI layer sees every event first and reports who owns it. When it
        // says `Ui`, the event stops here: the camera must not also orbit because the pointer
        // happened to be dragging a slider.
        //
        // Window-lifecycle events are exempt and always fall through. egui does not get to
        // swallow a close request, a resize, or a redraw — routing is about *input*, and
        // letting a UI state bug make the window unclosable is not a trade worth taking.
        let lifecycle = matches!(
            event,
            WindowEvent::CloseRequested
                | WindowEvent::Resized(_)
                | WindowEvent::RedrawRequested
                | WindowEvent::ScaleFactorChanged { .. }
        );
        if !lifecycle {
            if let Some(ui) = self.gfx.as_mut().and_then(|g| g.ui.as_mut()) {
                if ui.on_window_event(geometry, &event).target == PointerTarget::Ui {
                    // egui is retained-mode-ish about hover: it needs a frame to show the
                    // state change it just took.
                    return EventResponse::Redraw;
                }
            }
        }
        let mut response = EventResponse::Continue;
        match event {
            WindowEvent::CloseRequested => {
                self.finalize_recording(); // land an in-progress take, don't truncate it
                response = EventResponse::Exit;
            }
            WindowEvent::Resized(_) => self.on_resized(),
            WindowEvent::RedrawRequested => response = EventResponse::Redraw,
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.finalize_recording(); // land an in-progress take
                        response = EventResponse::Exit;
                    }
                    Key::Character(ref c) if c.eq_ignore_ascii_case("f") => {
                        // Intent only (#572 stage 3): the host reads `wants_fullscreen()` after
                        // each event and drives its own window.
                        self.fullscreen = !self.fullscreen;
                    }
                    Key::Character(ref c) if c.eq_ignore_ascii_case("h") => {
                        self.hdr.set_hdr(!self.hdr.hdr_enabled);
                    }
                    // #554 Tier 4: show/hide the in-window interface. Reached only when egui
                    // did not take the key, so typing "u" into a text field stays typing.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("u") => {
                        if let Some(ui) = self.gfx.as_mut().and_then(|g| g.ui.as_mut()) {
                            ui.visible = !ui.visible;
                        }
                    }
                    // Path tracer (#200 Tier 4): toggle the ground-truth mode. Reset
                    // the accumulation so it restarts clean.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("p") => {
                        self.pathtrace_on = !self.pathtrace_on;
                        self.pathtrace_spp = 0;
                    }
                    // Capture (#135): toggle the production-frame safe-area guide. The
                    // editor's Frame Guide checkbox drives it too (edge-detected each
                    // frame), so this is a convenience while lining up an OBS crop.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("g") => {
                        self.frame_guide = !self.frame_guide;
                    }
                    // Capture overlay (#135 P2): toggle the text overlay. The editor's
                    // Overlay checkbox drives it too (edge-detected each frame).
                    Key::Character(ref c) if c.eq_ignore_ascii_case("t") => {
                        self.overlay_on = !self.overlay_on;
                    }
                    // Capture decoration (#135 P5): master show/hide for axes + box.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("x") => {
                        self.decor_on = !self.decor_on;
                    }
                    // In-app recorder (#430): start/stop. The actual begin/end runs in
                    // render() (it knows the output size, format, and HDR state); this just
                    // latches the request. **R** = real-time capture (with audio); **Shift+R**
                    // = perfect / fixed-timestep capture (deterministic, perfectly smooth,
                    // video-only). The mode is latched here so the toggle picks it up at start.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("r") => {
                        self.record.toggle_pending = true;
                        // Only latch the mode when STARTING (not when stopping the current take).
                        if self.record.recorder.is_none() {
                            self.record.perfect_pending = self.mods_shift;
                        }
                    }
                    // #430: cycle the record length — Free (manual toggle) / 8 / 16 / 32 / 64
                    // bars. Only when idle, so the active take's auto-stop target can't change
                    // mid-recording. A transient toast surfaces the new selection.
                    //
                    // **Shift+B** cycles the chunk-mode *phrase* instead, in BEATS (4 / 8 /
                    // 16 / 32) — a different unit on purpose: a music-video cut is a phrase
                    // ("8 beats"), not a run length in bars.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("b") => {
                        if self.record.recorder.is_none() {
                            if self.mods_shift {
                                self.record.chunk_phrase_beats = next_phrase_beats(self.record.chunk_phrase_beats);
                                self.record.note = Some((
                                    format!("Chunk phrase: {:.0} beats", self.record.chunk_phrase_beats),
                                    std::time::Instant::now(),
                                ));
                            } else {
                                self.record.bars = next_record_bars(self.record.bars);
                                self.record.note = Some((
                                    format!("Record length: {}", record_len_label(self.record.bars)),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                    }
                    // #430 chunk mode: arm/disarm continuous phrase-aligned recording. The
                    // grid layout needs the live snapshot, so render() does the real work.
                    Key::Character(ref c) if c.eq_ignore_ascii_case("c") => {
                        self.record.chunk_arm_pending = true;
                    }
                    // #430: cycle the encoded file rate — 23.976 / 24 / 25 / 29.97 / 30 / 60.
                    // Idle only (the rate is baked into the running ffmpeg's arguments).
                    Key::Character(ref c) if c.eq_ignore_ascii_case("v") => {
                        if self.record.recorder.is_none() {
                            self.record.fps = recorder::next_fps(self.record.fps);
                            self.record.note = Some((
                                format!("Record rate: {}", self.record.fps.label()),
                                std::time::Instant::now(),
                            ));
                        }
                    }
                    Key::Character(ref c) if c.eq_ignore_ascii_case("o") => {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Radiance HDR", &["hdr"])
                            .pick_file()
                        {
                            let _ = std::fs::write(
                                ipc::hdr_sidecar_path(),
                                path.to_string_lossy().as_bytes(),
                            );
                            self.hdr.local_hdr_gen = self.hdr.local_hdr_gen.wrapping_add(1);
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseInput { button: MouseButton::Left, state, .. } => {
                self.dragging = state == ElementState::Pressed;
            }
            // #621 — the gesture arms **delegate**. They used to hold the orbit and zoom maths
            // themselves; both now live in `apply_camera_input`, so the visual and Organon
            // Mind's embedded viewport cannot orbit at different rates or clamp differently.
            // What stays here is winit's half: turning an absolute cursor position into the
            // delta the camera consumes, and a `MouseScrollDelta` into a scalar.
            WindowEvent::CursorMoved { position, .. } => {
                let (dx, dy) = (position.x - self.cursor.0, position.y - self.cursor.1);
                self.cursor = (position.x, position.y);
                if self.dragging {
                    self.apply_camera_input(CameraInput::Orbit { dx: dx as f32, dy: dy as f32 });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.apply_camera_input(CameraInput::Zoom { dy });
            }
            // Track Shift so Shift+R can select perfect (fixed-timestep) capture (#430).
            WindowEvent::ModifiersChanged(m) => {
                self.mods_shift = m.state().shift_key();
            }
            _ => {}
        }
        response
    }

    /// Move the camera — **the backend-neutral input seam** (#621).
    ///
    /// [`on_window_event`](World::on_window_event) above stays `winit::event::WindowEvent`-typed,
    /// exactly as #593 Tier 3 left it: it is the winit host's entry point and it carries the
    /// visual's whole keymap. This is the *second* entry point, and it carries only what a
    /// viewport needs to be a viewport. Two hosts drive it:
    ///
    /// - the **visual**, from `on_window_event`'s `CursorMoved` / `MouseWheel` arms, which now
    ///   translate and delegate rather than implementing the gesture themselves;
    /// - **Organon Mind's wgpu editor**, from `scene_input::SceneGesture`, which egui produces
    ///   inside `editor_ui` and `wgpu_editor` drains once per frame.
    ///
    /// Keeping the maths in one place is the point: the two rigs sit on the same desk, and an
    /// orbit that is subtly faster in one of them is the hardest kind of difference to notice
    /// and the easiest to ship. `scene_input::orbit_pixels` is the other half of that — it puts
    /// egui's points into the physical pixels this function has always been fed.
    ///
    /// **Not the keymap.** `scene_input`'s module docs carry why (most of it is projector work
    /// that means nothing in a docked pane, and **Esc** has an unresolved owner inside a plugin
    /// editor); widening the seam later is still open and nothing here forecloses it.
    pub fn apply_camera_input(&mut self, input: CameraInput) {
        match input {
            CameraInput::Orbit { dx, dy } => {
                if self.rails_ride {
                    // Rails (#187): drag steers the camera inside the bore (dragging right
                    // looks past geometry on the left, like leaning); clamped to the bore at
                    // the camera build.
                    let k = self.rails_bore * 0.004;
                    let max = self.rails_bore * 0.8;
                    self.rail_off.0 = (self.rail_off.0 + dx * k).clamp(-max, max);
                    self.rail_off.1 = (self.rail_off.1 - dy * k).clamp(-max, max);
                } else {
                    self.yaw -= dx * 0.01;
                    self.pitch = (self.pitch + dy * 0.01)
                        .clamp(-scene_input::PITCH_LIMIT, scene_input::PITCH_LIMIT);
                }
            }
            CameraInput::Zoom { dy } => {
                // Floor near 0 so you can zoom all the way through the centre (the near plane
                // is small enough that geometry stays visible as you arrive); ceiling unchanged.
                self.distance = (self.distance * (1.0 - dy * 0.001))
                    .clamp(scene_input::DISTANCE_MIN, scene_input::DISTANCE_MAX);
            }
            // Absolute framing (Console Spike, the portal's camera): the agent's shape of the
            // same gesture. `None` leaves an axis alone, so one message can move any subset.
            //
            // ⚠️ **The clamps are the SAME constants the two arms above use**, deliberately: an
            // agent and a hand must not disagree about where the instrument ends. Yaw has no
            // clamp because it has none for a drag either — it is an angle and the trigonometry
            // wraps; the command lane bounds it (`scene_input::YAW_LIMIT`) so that a schema can
            // state a range, not because the world cannot hold the value.
            //
            // ⚠️ Non-finite is dropped rather than clamped. `f32::clamp` **panics** on a NaN
            // bound and quietly returns NaN for a NaN input, and a NaN yaw poisons the whole
            // view matrix — a black window with no error, which is the worst of both.
            //
            // 📌 Written even while `rails_ride` is on, where the finalization ignores all three.
            // The alternative is a refusal that would have to travel back up a lane with no
            // return path; writing means the framing is simply there when the ride ends.
            CameraInput::Frame { yaw, pitch, distance } => {
                if let Some(y) = yaw.filter(|v| v.is_finite()) {
                    self.yaw = y;
                }
                if let Some(p) = pitch.filter(|v| v.is_finite()) {
                    self.pitch = p.clamp(-scene_input::PITCH_LIMIT, scene_input::PITCH_LIMIT);
                }
                if let Some(d) = distance.filter(|v| v.is_finite()) {
                    self.distance =
                        d.clamp(scene_input::DISTANCE_MIN, scene_input::DISTANCE_MAX);
                }
            }
        }
    }

    /// Where the viewer stands right now: the base orbit's `(yaw, pitch, distance)`.
    ///
    /// 🚨 **The read half of [`apply_camera_input`](World::apply_camera_input), and it must be
    /// this rather than a copy the host keeps.** All four writers land on these three fields — a
    /// drag, a wheel, `organon console camera`, and an MCP framing — and the world *clamps* on
    /// the way in. A host that remembered what it last asked for would report a value the camera
    /// may never have held, and would be blind to every move a hand made. The console serves this
    /// to an agent (`organon-console::camera::Viewpoint`), and an agent acting on a stale framing is
    /// exactly the failure that read exists to end.
    ///
    /// ⚠️ **It is the base orbit, not the camera the frame is drawn with.** The finalization adds
    /// `cam_path`'s auto-orbit offset on top, and an installed [`set_substrate_rig`] overrides all
    /// six values wholesale — so this is what a framing command *writes*, which is the question a
    /// caller computing a delta is actually asking. `camera::viewpoint_is_visible` is how the
    /// console says whether anything on screen is showing it.
    // Dead in `bin/visual.rs`, which `#[path]`-includes this file and calls none of the world's
    // host-facing accessors — same reason `set_substrate_rig` and `queue()` carry the allow.
    #[allow(dead_code)]
    pub fn camera_framing(&self) -> (f32, f32, f32) {
        (self.yaw, self.pitch, self.distance)
    }

    /// Install (or clear) an **absolute** camera rig — Console Spike Tier 1, for Organon
    /// the Console's substrate backdrop.
    ///
    /// The tuple is `(center, yaw, pitch, distance, roll, fov_deg)` — the six the camera
    /// finalization selects between, in that order, which is exactly what
    /// `substrate_camera::SubstrateRig::camera_arm` returns. `Some` overrides all six for as
    /// long as it is set and **latches off the `cam_center` auto-follow** (the 5 %/frame lerp
    /// toward the generator field's AABB centre, which a flat backdrop plane must not be
    /// dragged by); `None` hands the camera back to the orbit/rails rig with nothing left
    /// behind.
    ///
    /// **Absolute is the whole point.** [`apply_camera_input`](World::apply_camera_input) is
    /// the relative API — deltas onto `yaw`/`pitch`/`distance` — and framing a plane off
    /// ratcheted deltas is not a rig. Call this again to re-frame; the caller owns the
    /// arithmetic, and it must re-frame when the render target's **aspect** changes, since the
    /// engine reads aspect from the target every frame and a rig computed for another one
    /// stops covering the viewport.
    // Dead in `bin/visual.rs`, which `#[path]`-includes this file and calls neither this nor
    // the world's other host-facing setters — same reason `queue()` carries the allow.
    #[allow(dead_code)]
    pub fn set_substrate_rig(&mut self, rig: Option<(Vec3, f32, f32, f32, f32, f32)>) {
        self.substrate_rig = rig;
    }

    /// Whether the world currently wants to be fullscreen (**F**). The host applies it — a
    /// winit window via `set_fullscreen`, an embedded pane by ignoring it (#572 stage 3).
    pub fn wants_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Seed the fullscreen state at launch, when the host opened its window borderless-fullscreen
    /// on a picked display. Without it **F** would need two presses to leave fullscreen.
    pub fn set_fullscreen_state(&mut self, on: bool) {
        self.fullscreen = on;
    }

    /// The wgpu device the world is running on, once [`attach_gpu`](World::attach_gpu) has been
    /// called. The host needs it to configure its own surface — it created the device, but the
    /// world owns it, and there is no reason for the host to keep a second handle in sync.
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.gfx.as_ref().map(|g| &g.device)
    }

    /// The wgpu queue, on the same terms as [`device`](World::device).
    ///
    /// Added by #593 Tier 2: a host that draws its **own** late pass over the frame — the Mind
    /// editor's egui layer — needs to upload egui's textures and submit its encoder on the
    /// world's queue. Drawing it on a second queue is not an option; it is the same device and
    /// the same swapchain image, written after [`render_into`](World::render_into) returns.
    /// (The winit host does not need this: its UI pass runs *inside* the frame via `ui_layer`.)
    // Same shape as `UiEvent::response`: the reader is `wgpu_editor.rs` in the library, and
    // `bin/visual.rs`'s `#[path]` copy has none — its UI pass runs *inside* the frame via
    // `ui_layer`, so it never needs the queue back out.
    #[allow(dead_code)]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.gfx.as_ref().map(|g| &g.queue)
    }

    /// Present a swapchain image the host acquired. Goes through the world only because wgpu 30
    /// moved `present` onto the **queue**, which lives with the device.
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.queue.present(frame);
        }
    }
}

/// Environment tint colour from a hue (degrees) + amount (0..1 saturation).
/// `amount = 0` → white (no tint); higher pulls the colour toward the hue at
/// `value = 1`, so it shifts hue without dimming the brightest channel.
fn env_tint_rgb(hue_deg: f32, amount: f32) -> [f32; 3] {
    let s = amount.clamp(0.0, 1.0);
    let h = (hue_deg.rem_euclid(360.0)) / 60.0;
    let c = s; // value = 1, chroma = value * sat = sat
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = 1.0 - c; // lift so value (max channel) stays at 1.0
    [r + m, g + m, b + m]
}

/// World-space unit direction TO a light from elevation/azimuth in degrees.
/// Elevation 0° = horizon, +90° = straight up; azimuth sweeps around +Y.
fn dir_from_angles(elevation_deg: f32, azimuth_deg: f32) -> Vec3 {
    let e = elevation_deg.to_radians();
    let a = azimuth_deg.to_radians();
    Vec3::new(e.cos() * a.sin(), e.sin(), e.cos() * a.cos()).normalize_or_zero()
}

/// Cast-shadow (#152 Tier 3) light view-projection: an orthographic frustum from
/// `dir` (direction TO the key light) looking at the scene centre, fit to the
/// bounds' 8 corners in LIGHT space. (The old sphere fit — half-diagonal padded
/// another 1.5× — left the geometry covering only ~15% of the 2048² map in the
/// worst axis, an effective ~800² shadow resolution.) The frustum origin is then
/// snapped to shadow-texel increments so edges don't shimmer as the camera/light
/// drifts sub-texel (the snap is partial on a breathing AABB, whose texel size
/// itself changes). wgpu depth convention (`orthographic_rh`, z ∈ [0,1]) so it
/// matches the shadow shader's `ndc.z`.
fn shadow_light_matrix(dir: Vec3, bmin: Vec3, bmax: Vec3) -> [[f32; 4]; 4] {
    // Keep in sync with shadow.rs::SHADOW_RES (the module is private to render.rs).
    const SHADOW_RES: f32 = 2048.0;
    let center = (bmin + bmax) * 0.5;
    let radius = ((bmax - bmin).length() * 0.5).max(0.5);
    let mut d = dir.normalize_or_zero();
    if d.length_squared() < 1e-6 {
        d = Vec3::Y;
    }
    let eye = center + d * (radius * 2.0 + 1.0);
    let up = if d.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    let view = Mat4::look_at_rh(eye, center, up);
    let mut lmin = Vec3::splat(f32::INFINITY);
    let mut lmax = Vec3::splat(f32::NEG_INFINITY);
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { bmin.x } else { bmax.x },
            if i & 2 == 0 { bmin.y } else { bmax.y },
            if i & 4 == 0 { bmin.z } else { bmax.z },
        );
        let lc = view.transform_point3(corner);
        lmin = lmin.min(lc);
        lmax = lmax.max(lc);
    }
    let pad = ((lmax - lmin).max_element() * 0.02).max(0.05);
    let w = (lmax.x - lmin.x) + 2.0 * pad;
    let h = (lmax.y - lmin.y) + 2.0 * pad;
    let tx = w / SHADOW_RES;
    let ty = h / SHADOW_RES;
    let l = ((lmin.x - pad) / tx).floor() * tx;
    let b = ((lmin.y - pad) / ty).floor() * ty;
    // The view looks down −Z: points in front have negative z, so the near plane
    // distance is −lmax.z and the far −lmin.z.
    let near = (-lmax.z - pad).max(0.01);
    let far = -lmin.z + pad;
    let proj = Mat4::orthographic_rh(l, l + w, b, b + h, near, far);
    (proj * view).to_cols_array_2d()
}

/// Dynamic-resolution controller: nudge the render `scale` toward the one that
/// hits `target_ms` per frame. Render cost ∝ pixels ∝ scale², so the ideal scale
/// is `scale·√(target/frame)`; we step 25% toward it (smooth, no oscillation) and
/// hold inside a ±10% deadband around the target. Clamped to 0.25..1.0.
/// Minimal extractor for a flat string field in the overlay sidecar JSON
/// (`{ "handle": "…", "title": "…" }`) — avoids a serde_json dep in the visual.
/// Returns the (basically-unescaped) value for `"key": "value"`, or None.
fn json_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let after = &json[json.find(&pat)? + pat.len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let q0 = rest.find('"')? + 1;
    let body = &rest[q0..];
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(match n {
                        'n' => '\n',
                        't' => '\t',
                        other => other, // \" \\ \/ → literal
                    });
                }
            }
            _ => out.push(c),
        }
    }
    Some(out)
}

/// Halton low-discrepancy sequence (base `b`, 1-based index) — the standard TAA
/// jitter pattern (#174 T3): 8 frames of (2,3) offsets cover the pixel evenly.
fn halton(mut i: u32, b: u32) -> f32 {
    let mut f = 1.0f32;
    let mut r = 0.0f32;
    while i > 0 {
        f /= b as f32;
        r += f * (i % b) as f32;
        i /= b;
    }
    r
}

fn drs_adjust(scale: f32, frame_ms: f32, target_ms: f32) -> f32 {
    let (lo, hi) = (target_ms * 0.9, target_ms * 1.1);
    if frame_ms >= lo && frame_ms <= hi {
        return scale.clamp(0.25, 1.0);
    }
    let ideal = scale * (target_ms / frame_ms.max(0.1)).sqrt();
    (scale + (ideal - scale) * 0.25).clamp(0.25, 1.0)
}

/// Build the terrain backdrop uniforms. The ray DIRECTIONS come from the real
/// orbit camera (via `sky.inv_view_proj`, where translation cancels), so the
/// horizon tracks it; the ray ORIGIN is a synthetic fly-camera that rides above
/// the landscape on a bounded cosine drift (endless varied terrain without float
/// drift). `fly_time` is the fly clock (already scaled by the fly speed).
fn build_terrain_uniforms(
    s: &ipc::Shared,
    sky: &render::SkyUniforms,
    noise: &[f32],
    fly_time: f64,
    sun_elev: f32,
) -> render::TerrainUniforms {
    let te = &s.terrain;
    let height = te[1];
    let ridged = te[11] != 0.0;
    let ride = te[8];
    let drift_r = 1100.0_f64;
    let fx = ((0.23 * fly_time).cos() * drift_r) as f32;
    let fz = ((1.5 + 0.21 * fly_time).cos() * drift_r) as f32;
    let ground = render::terrain_height(noise, fx, fz, height, ridged);
    let sun = dir_from_angles(sun_elev, te[5]);
    // Sea level as a fraction (te[21]) of the terrain height (world y).
    let water_y = te[21] * height;
    // #102B: in ocean-only mode (landscape off, ocean on) the fly camera rides above
    // the sea level instead of above the (hidden) terrain.
    let land_on = te[0] != 0.0;
    let ocean_on = s.ocean[0] != 0.0;
    let eye_y = if land_on || !ocean_on { ground + ride } else { s.ocean[1] + ride };
    render::TerrainUniforms {
        inv_view_proj: sky.inv_view_proj,
        ro: [fx, eye_y, fz, 0.0],
        sun: [sun.x, sun.y, sun.z, te[6]],
        p0: [height, te[2], te[3], if ridged { 1.0 } else { 0.0 }],
        p1: [te[12], te[13], fly_time as f32, 0.0],
        // march steps, march octaves, resolution divisor (fs_blit), palette id.
        p2: [te[14], te[15], te[16].max(1.0), te[17]],
        // emissive, scatter, godray, water_on.
        p3: [te[18], te[24], te[25], te[20]],
        // water level (world y), water hue, water ripple, _.
        p4: [water_y, te[22], te[23], 0.0],
        // #100 atmosphere: [enabled, turbidity, mie_g, sun_intensity] /
        // [ground_albedo, exposure, aerial_strength, rayleigh]. The sun direction is
        // already in `sun` (terrain sun elev/azim, the day cycle).
        atmos: [s.atmosphere[0], s.atmosphere[1], s.atmosphere[2], s.atmosphere[3]],
        atmos2: [s.atmosphere[4], s.atmosphere[5], s.atmosphere[6], s.atmosphere[7]],
        // #102 clouds: [enabled, coverage, density, base] / [thickness, steps, detail,
        // drift] / [hg, absorption, shadow, ambient].
        clouds: [s.clouds[0], s.clouds[1], s.clouds[2], s.clouds[3]],
        clouds2: [s.clouds[4], s.clouds[5], s.clouds[6], s.clouds[7]],
        clouds3: [s.clouds[8], s.clouds[9], s.clouds[10], s.clouds[11]],
        // #102B ocean: [enabled, level, foam, glitter] / [hue, depth, tile_size,
        // land_on]. land_on lets the shader skip the terrain for an ocean-only world.
        ocean: [s.ocean[0], s.ocean[1], s.ocean[7], s.ocean[8]],
        ocean2: [s.ocean[9], s.ocean[10], s.ocean[6], if land_on { 1.0 } else { 0.0 }],
    }
}

/// Build the starfield uniforms. The star directions come from the embedded
/// catalog (fixed equatorial unit vectors); this assembles the equatorial→world
/// rotation `R(latitude) · Rz(sidereal)`, the night factor (stars fade in as the
/// day-cycle sun sets), and the HDR sun disc (riding the same sun direction). Ray
/// projection reuses the *unscaled* scene view-projection (inverted back from
/// `sky.inv_view_proj`) so the sky stays put while the scene breathes against it.
fn build_star_uniforms(
    s: &ipc::Shared,
    sky: &render::SkyUniforms,
    sky_time: f64,
    wall_time: f64,
    sun_elev: f32,
    size: (u32, u32),
) -> render::StarUniforms {
    let st = &s.stars;
    // Unscaled scene view-projection (project star points-at-infinity).
    let view_proj = Mat4::from_cols_array_2d(&sky.inv_view_proj).inverse();

    // Equatorial → world: place the north celestial pole at altitude = latitude
    // toward +Z (north, the same axis the sun azimuth is measured from), with the
    // meridian equator point to the south and east→−X. Columns are the world images
    // of the equatorial basis (e_x = equinox, e_y, e_z = NCP).
    let phi = st[5].to_radians();
    let (sp, cp) = (phi.sin(), phi.cos());
    let r_world = Mat3::from_cols(
        Vec3::new(0.0, cp, -sp), // e_x → meridian equator point
        Vec3::new(-1.0, 0.0, 0.0), // e_y → west
        Vec3::new(0.0, sp, cp),  // e_z → NCP at altitude φ, north
    );
    // Daily/sidereal spin about the NCP (equatorial z).
    let rot = r_world * Mat3::from_rotation_z(sky_time as f32);

    // Night factor: 1 once the sun is well below the horizon, 0 in daylight.
    let night = 1.0 - smoothstep(-6.0, 8.0, sun_elev);
    // Sun visibility: fades out as it dips below the horizon.
    let sun_day = smoothstep(-2.0, 3.0, sun_elev);

    // Sun direction shares the terrain sun elevation/azimuth (the day cycle).
    let sun_dir = dir_from_angles(sun_elev, s.terrain[5]);
    // Angular radius (deg) → vertical NDC radius for a 45° vertical FOV.
    let half_fov = 22.5_f32.to_radians();
    let sun_ndc_r = (st[11].to_radians().tan() / half_fov.tan()).max(0.0);
    // Warmth tint: white → deep sunset orange.
    let warmth = st[12].clamp(0.0, 1.0);
    let sun_col = Vec3::new(1.0, 1.0, 1.0).lerp(Vec3::new(1.0, 0.55, 0.22), warmth);
    let sun_bright = st[10] * sun_day;

    render::StarUniforms {
        view_proj: view_proj.to_cols_array_2d(),
        rot: Mat4::from_mat3(rot).to_cols_array_2d(),
        params: [st[1], st[4], night, st[7]],
        twinkle: [st[2], st[3], wall_time as f32, st[8]],
        viewport: [size.0 as f32, size.1.max(1) as f32, 0.0, 0.0],
        sun: [sun_dir.x, sun_dir.y, sun_dir.z, sun_ndc_r],
        sun_color: [sun_col.x, sun_col.y, sun_col.z, sun_bright],
    }
}

/// Hermite smoothstep (e0 < e1).
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scene camera projection planes — shared by `build_uniforms` and the DoF
/// focus-distance remap (FxParams), which linearizes the prepass depth with them.
const CAM_NEAR: f32 = 0.1;
const CAM_FAR: f32 = 5000.0;
/// Rails (#187): how far down the corridor the forward camera's look-at target
/// sits. Only sets the view direction (the eye stays at the bore offset).
const RAILS_LOOK_AHEAD: f32 = 60.0;

#[allow(clippy::too_many_arguments)]
/// #472 Tier 5: inject a time term into a baked layer so the material flows/evolves.
/// `live` = `Shared.material_live` (`[anim_on, speed, mode, flow_x, flow_y, …]`); `t`
/// is wall-time seconds. Returns a copy with the layer's offset (`[4]`,`[5]`) or
/// rotation (`[3]`) advanced. Off (`live[0] = 0`) → the layer is unchanged.
fn animate_layer(mut l: [f32; 18], live: &[f32; 8], t: f32) -> [f32; 18] {
    if live[0] < 0.5 {
        return l;
    }
    let phase = t * live[1]; // speed
    match live[2] as u32 {
        2 => l[3] += phase,                               // Rotate: field rotation
        1 => {
            l[4] += (phase).sin() * 0.5; // Evolve: circular churn of the offset
            l[5] += (phase).cos() * 0.5;
        }
        _ => {
            l[4] += live[3] * phase; // Drift: pan along the flow direction
            l[5] += live[4] * phase;
        }
    }
    l
}

fn build_uniforms(
    s: &ipc::Shared,
    center: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    size: (u32, u32),
    breath_scale: Vec3,
    // Effective terrain sun elevation (deg) for the day cycle. When the terrain
    // backdrop is on with "sun lights scene", the generator's key light follows it.
    terrain_sun_elev: f32,
    // Beat-clock phase (#305 Tier 1): drives the material hue auto-cycle.
    beat: f32,
    // #307 Tier 2: camera roll (dutch, radians) rolls the up-vector; `fov_deg` is the
    // vertical FOV (45 = today; drives the dolly-zoom).
    roll: f32,
    fov_deg: f32,
    // #472 Tier 1: runtime bitfield of which material PNG channel maps the visual has
    // loaded (0 when no folder is loaded → the cube shader keeps the scalar path).
    material_present_mask: f32,
) -> (
    render::Uniforms,
    render::SkyUniforms,
    render::PostParams,
    render::SsaoParams,
    render::SsrParams,
    render::SsgiParams,
) {
    let dir = Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos());
    let eye = center + distance * dir;
    // Roll the up-vector about the view axis for a dutch tilt (#307 Tier 2). Roll 0
    // → plain Vec3::Y (today's look). Rodrigues rotation of Y about the (eye→center)
    // view direction.
    let up = if roll.abs() > 1.0e-6 {
        let k = (center - eye).normalize_or_zero(); // view axis
        let y = Vec3::Y;
        let (s_r, c_r) = roll.sin_cos();
        (y * c_r + k.cross(y) * s_r + k * (k.dot(y)) * (1.0 - c_r)).normalize_or_zero()
    } else {
        Vec3::Y
    };
    let view = Mat4::look_at_rh(eye, center, up);
    let aspect = (size.0 as f32 / size.1.max(1) as f32).max(0.01);
    // Near plane 0.1 (was 1.0) so zooming to the centre doesn't clip geometry off
    // right as you arrive; far 5000 keeps the skybox. The wider ratio costs some
    // depth precision but the scene sits near the origin, so it's not noticeable.
    // The SECOND of the two FOV clamps (the other is at the camera finalization). 4°, widened
    // from 10° by the Console Spike's Tier 1 for the substrate backdrop's long lens; both had
    // to move or neither did.
    let proj = Mat4::perspective_rh(fov_deg.clamp(4.0, 120.0).to_radians(), aspect, CAM_NEAR, CAM_FAR);
    let view_proj = proj * view;
    // The sky reconstructs ray directions from the *unscaled* inverse, so the
    // backdrop stays put while the scene breathes against it.
    let inv_view_proj = view_proj.inverse();
    // Breath: scale the scene about its centre at the view level — applied to the
    // cube/membrane/metaball pass only (the sky uniform above is untouched), so a
    // single matrix makes every generator + surface mode swell on the pulse. Lighting
    // reads unscaled world positions (the model matrices don't change), which is
    // imperceptible for the subtle scales this drives. Vec3::ONE → identity (inert).
    let scene_view_proj = view_proj
        * Mat4::from_translation(center)
        * Mat4::from_scale(breath_scale)
        * Mat4::from_translation(-center);

    let metallic = s.pbr[0];
    let roughness = s.pbr[1].clamp(0.0, 1.0);
    // pbr[2] is exposure in EV stops → linear gain. Pulse no longer auto-pumps
    // exposure/glow; it only acts where the user routes it (Pulse Routing /
    // Speed Pulse), which is already baked into `s` before this point.
    let exposure = 2.0f32.powf(s.pbr[2]);
    let env_intensity = s.pbr[3];
    let env_rotation = s.pbr[4].to_radians();
    let bloom_intensity = s.pbr[5];
    let bloom_threshold = s.pbr[6];

    // Direct-lighting controls (reuse the existing key/fill/elevation/azimuth
    // params, formerly vestigial under the pure-IBL renderer).
    let ambient = s.lighting[0];
    let mut key_intensity = s.lighting[1];
    let fill_intensity = s.lighting[2];
    let mut key_dir = dir_from_angles(s.lighting[3], s.lighting[4]);
    // Time-of-day: when the terrain backdrop is on with "sun lights scene", the
    // generator's key light follows the terrain sun (and dims toward night).
    if s.terrain[0] != 0.0 && s.terrain[26] > 0.5 {
        key_dir = dir_from_angles(terrain_sun_elev, s.terrain[5]);
        let h = terrain_sun_elev.to_radians().sin().max(0.0);
        key_intensity = s.lighting[1] * (0.15 + 0.85 * h);
    }
    // Fill: a softer wrap light from roughly the opposite side, near the horizon.
    let fill_dir = dir_from_angles(10.0, s.lighting[4] - 120.0);
    let glow = s.lighting[5];
    let opacity = s.lighting[6];
    let mat_type = s.lighting[7]; // 0=Standard, 1=Chrome, 2=Glass, 3=Refractive
    let ior = s.pbr[7].max(1.0); // glass index of refraction

    // Environment tint (hue + amount): a normalized colour that multiplies the
    // env contribution (white when amount = 0). Background brightness folds the
    // visibility flag in so the backdrop can hide while the IBL keeps lighting.
    let tint = env_tint_rgb(s.env_tint_hue, s.env_tint_amt);
    let bg_brightness = if s.bg_visible != 0 { s.bg_intensity } else { 0.0 };

    // Neural Tissue membrane (#260 Tier 1): a waxy translucent membrane driven by
    // the neural_surface material dials, layered onto the shared Surface-FX
    // translucency/iridescence path so no new shader pipeline is needed. Only when
    // the Neural Tissue surface is active, and inert at the default dials (0).
    let (nt_sss, nt_irid) = if s.surface_mode == 7 {
        (s.neural_surface[3], s.neural_surface[4])
    } else {
        (0.0, 0.0)
    };

    // Scene shaders output LINEAR HDR; exposure/tonemap live in the composite,
    // so the cube/sky uniforms carry no exposure (env.x kept for layout only).
    let uniforms = render::Uniforms {
        view_proj: scene_view_proj.to_cols_array_2d(),
        camera_pos: [eye.x, eye.y, eye.z, 0.0],
        // mat[3] (prefilter_mip_count) is overwritten by Renderer::render.
        mat: [metallic, roughness, glow, 0.0],
        env: [1.0, env_intensity, env_rotation, opacity],
        key_light: [key_dir.x, key_dir.y, key_dir.z, key_intensity],
        fill_light: [fill_dir.x, fill_dir.y, fill_dir.z, fill_intensity],
        // amb.w = palette-active flag: when an explicit LUT is selected, the
        // shader uses the per-instance tint AS the albedo (replacing the RGB cube).
        amb: [ambient, mat_type, ior, if s.surface_fx[6] >= 0.5 { 1.0 } else { 0.0 }],
        // Additive surface modifiers (translucency + iridescence). Both inert at
        // amount 0, so the look is unchanged until dialed up.
        sss: [s.surface_fx[0] + nt_sss, s.surface_fx[1] + nt_sss, s.surface_fx[2] + nt_sss, 0.0],
        irid: [s.surface_fx[3] + nt_irid, s.surface_fx[4] + nt_irid, s.surface_fx[5] + nt_irid, 0.0],
        // env_tint.w carries the Material Emissive (HDR self-emission in the surface's
        // own colour; cube.wgsl fs_main reads it). 0 → glow-only, byte-identical.
        env_tint: [tint[0], tint[1], tint[2], s.emissive[0]],
        // Emissive ripple: intensity/freq/sharp/geom from params; the live phase
        // and field centre/radius are patched in by `render()` (which owns the
        // phase clock + the field bounds). Axial axis = world +Y.
        ripple: [s.bio[1], 0.0, s.bio[3], s.bio[4].max(1.0)],
        ripple_ctr: [center.x, center.y, center.z, 0.0],
        ripple_mode: [s.bio[5], 0.0, 1.0, 0.0],
        // Spectral glass (#80 C): dispersion / caustic / thin-film / spectral count.
        // All 0 → today's single-IOR glass exactly.
        glassx: [s.glass_spec[0], s.glass_spec[1], s.glass_spec[2], s.glass_spec[3]],
        // Reflection controls (#163 Tier 1): palette influence + chrome purity /
        // glass clarity / Standard reflectivity override. All 0 → today's look.
        reflect_ctl: [s.reflect[0], s.reflect[1], s.reflect[2], s.reflect[3]],
        // Reflection probe / parallax (#163 Tier 2): default OFF (source 0). The real
        // AABB is patched in below where the field `bounds` are known; source 0 here
        // means the cube shader ignores the box → today's reflection.
        refl_box_min: [0.0, 0.0, 0.0, 0.0],
        refl_box_max: [0.0, 0.0, 0.0, 0.0],
        // Refractive material: Beer–Lambert absorption strength (read by the
        // cube shader when mat_type = 3), plus the refraction-overlay
        // enable/blend that weave the same optics into the other materials.
        refr: [
            s.refrmat[0].max(0.0),
            s.refrmat[1],
            s.refrmat[2].clamp(0.0, 1.0),
            0.0,
        ],
        // Anisotropy (#214 T1): amount / brush rotation (deg→rad) / overlay
        // enable+blend. All 0 → isotropic; read by the cube shader (and rt_reflect).
        aniso: [
            s.aniso[0].clamp(-1.0, 1.0),
            s.aniso[1].to_radians(),
            s.aniso[2],
            s.aniso[3].clamp(0.0, 1.0),
        ],
        // Surface lobes (#214 T2): clearcoat [strength, roughness, overlay-enable,
        // sheen-overlay-enable] + sheen [amount, roughness, tint]. All 0 → today.
        coat: [
            s.coat[0].clamp(0.0, 1.0),
            s.coat[1].clamp(0.0, 1.0),
            s.coat[2],
            s.coat[3],
        ],
        sheen: [
            s.coat[4].clamp(0.0, 1.0),
            s.coat[5].clamp(0.0, 1.0),
            s.coat[6].clamp(0.0, 1.0),
            0.0,
        ],
        // Body optics (#214 T3): SSS thickness drive / radius / interior scatter.
        // All 0 → today's look; read by the cube shader only.
        body: [
            s.body[0].clamp(0.0, 1.0),
            s.body[1].max(0.05),
            s.body[2].clamp(0.0, 1.0),
            0.0,
        ],
        // Microstructure (#214 T4): glitter [amount, density, sharpness] +
        // diffraction [amount, freq] + retro [amount]. All 0 → today's look.
        micro: [
            s.micro[0].clamp(0.0, 1.0),
            s.micro[1].max(0.1),
            s.micro[2].clamp(0.0, 1.0),
            s.micro[3].clamp(0.0, 1.0),
        ],
        micro2: [
            s.micro[4].max(1.0),
            s.micro[5].clamp(0.0, 1.0),
            0.0,
            0.0,
        ],
        // Spectral emission (#214 T5 pt 1): fluorescence / hue / incandescence /
        // temperature (K). All amounts 0 → today's look.
        emit: [
            s.emit[0].clamp(0.0, 1.0),
            s.emit[1].clamp(0.0, 1.0),
            s.emit[2].clamp(0.0, 1.0),
            s.emit[3].max(1000.0),
        ],
        // Physical thin-film (#258 T1): base thickness (nm) / marbling / film IOR /
        // drainage. thickness 0 → the shader keeps the cosine-hack path (byte-identical).
        thinfilm: [
            s.thinfilm[0].max(0.0),
            s.thinfilm[1].clamp(0.0, 1.0),
            s.thinfilm[2].max(1.0),
            s.thinfilm[3].clamp(0.0, 1.0),
        ],
        // Demo point light (#288 Tier 3): off by default; the Demo generator patches
        // it from the brightest scene emitter after this returns (render loop).
        demo_light_pos: [0.0, 0.0, 0.0, 0.0],
        demo_light_col: [0.0, 0.0, 0.0, 0.0],
        // #305 Tier 1: generator material HSV — effective hue = base + cycle·beat.
        matcol: [s.matcol[0] + s.matcol[1] * beat, s.matcol[2], s.matcol[3], 0.0],
        // #305 Tier 2: live-sky cloud reflection — drift phase = speed·beat.
        skyrefl: [s.skyrefl[0], s.skyrefl[1], s.skyrefl[2] * beat, s.skyrefl[3]],
        // #349 Tier 1: calibrated colour law. [mode, lut, amount, cal_t]. The measured
        // level → LUT coord is resolved CPU-side (`calibrated_colour_t`) so the shader
        // just samples + blends. mode 0 (Aesthetic) → `apply_calibrated` is a no-op →
        // byte-identical.
        cal: [
            s.colour[0], // mode (ColourMode wire value)
            s.colour[3], // lut (CalLut wire value)
            s.colour[5], // amount (0..1)
            calibrated_colour_t(s),
        ],
        // Node bevel: rounds the cube geometry (cube→sphere) in the vertex shader.
        // render() zeros this for every draw except the Original / Flow-Aligned
        // instanced-cube draw, so only the generator's cubes round. 0 → sharp cube.
        shape: [s.bevel, 0.0, 0.0, 0.0],
        // #472 Tier 1 materials. render() zeros mtl.x for every draw except the
        // generator cube draw, so only the generator's cubes sample the texture set.
        // present_mask (mtl2.w) is the visual's runtime record of which PNG channel
        // maps actually loaded (0 when no folder loaded → the shader keeps the scalar
        // path even if mtl.x were on). mtl.x = 0 → byte-identical.
        mtl: [
            // material_on: the Tier-1 texture set OR the Tier-2 procedural bake.
            if s.material[0] > 0.5 || s.material_layer[16] > 0.5 { 1.0 } else { 0.0 },
            s.material[1],       // projection_mode
            s.material[2],       // scale
            s.material_live[5],  // #472 Tier 5: height→vertex displacement amount
                                 // (was the removed normal_strength slot; the unified
                                 // shader perturbs the normal at full strength).
        ],
        mtl2: [
            s.material[4],        // ao_strength
            s.material[5],        // rough_scale
            s.material[6],        // metal_scale
            material_present_mask, // runtime bitfield of loaded maps
        ],
    };
    let sky_uniforms = render::SkyUniforms {
        inv_view_proj: inv_view_proj.to_cols_array_2d(),
        cam_pos: [eye.x, eye.y, eye.z, 0.0],
        params: [1.0, env_intensity, env_rotation, bg_brightness],
        env_tint: [tint[0], tint[1], tint[2], 0.0],
    };
    let ao_enabled = s.ssao[0];
    let post_params = render::PostParams {
        exposure,
        bloom_intensity,
        bloom_threshold,
        hdr_max: 1.0, // SDR by default; render() overrides when HDR output is on
        hdr_knee: s.hdr_knee.clamp(0.1, 1.0),
        tonemap: s.tonemap as f32,
        bg_tonemap: s.bg_tonemap as f32,
        ao_enabled,
        ao_intensity: s.ssao[2],
        gamut: 0.0, // set by render() (needs hdr_enabled + hdr_wide)
        vivid: s.hdr_vivid.clamp(0.0, 1.0),
        time: 0.0, // set by render() (the SDR dither clock, #174 T3)
        // Learned upscaler (#200 Tier 5c): enable flag → mode; render() zeroes it
        // when render_scale >= ~1 (nothing to upscale). sharpen strength + seed.
        up_mode: if s.upscale[0] != 0.0 { 1.0 } else { 0.0 },
        up_sharpen: s.upscale[1].max(0.0),
        up_seed: s.upscale[2].max(0.0),
    };
    // SSAO works in view space: reconstruct from depth via inv_proj, project
    // samples back with proj. (radius, intensity, bias) ride params; intensity is
    // applied composite-side, kept here for completeness.
    let texel = [
        1.0 / size.0.max(1) as f32,
        1.0 / size.1.max(1) as f32,
        size.0 as f32,
        size.1 as f32,
    ];
    let ssao = render::SsaoParams {
        proj: proj.to_cols_array_2d(),
        inv_proj: proj.inverse().to_cols_array_2d(),
        params: [s.ssao[1], s.ssao[2], s.ssao[3], 0.0],
        texel,
    };
    // SSR (#80 A): same view-space reconstruction as SSAO; carries the global
    // material (metallic/roughness/type) since this renderer has no G-buffer.
    let ssr = render::SsrParams {
        proj: proj.to_cols_array_2d(),
        inv_proj: proj.inverse().to_cols_array_2d(),
        mat: [metallic, roughness, mat_type, 0.0],
        ssr: [s.ssr[1], s.ssr[2], s.ssr[3], 0.0], // intensity, max_roughness, thickness
        perf: [s.ssr[4], s.ssr[5], 0.0, 0.0],     // max_steps, stride
        texel,
    };
    // SSGI (#152 Tier 2): same view-space reconstruction; gathers one diffuse
    // bounce. `extra[1]` (frame seed) is patched per-frame in render().
    let ssgi = render::SsgiParams {
        proj: proj.to_cols_array_2d(),
        inv_proj: proj.inverse().to_cols_array_2d(),
        params: [s.ssgi[1], s.ssgi[2], 8.0, s.ssgi[3]], // intensity, radius, steps, rays
        extra: [s.ssgi[2] * 0.5, 0.0, 0.0, 0.0],        // thickness, frame_seed
        texel,
    };
    (uniforms, sky_uniforms, post_params, ssao, ssr, ssgi)
}

/// Time constant (seconds) for the phase-locked loop pulling the visual's beat
/// clock toward the host. ~0.12s feels locked without snapping (no jitter).
const PLL_TAU: f64 = 0.12;

/// The current pulse envelope (≈0..1, peaking on a hit and decaying between).
/// `pulse_source == 1` uses the live audio bass band (already attack/release-
/// smoothed by the plugin); otherwise it's the synthetic decaying beat impulse
/// off the PLL clock. Downstream routing + the exposure/glow pump don't care
/// which source is active — both feed this one envelope. The audio value is
/// clamped so a hot input can't blow the modulation up.
/// Jellyfish contraction stroke over a unit phase `x` ∈ [0,1): a smooth squeeze to
/// full by `peak` (~30%) then a slower recovery back to rest. Built from two
/// half-cosine segments so it is C¹-continuous everywhere — including across the
/// phase wrap (slopes are 0 at x=0, the peak, and x=1) — which is what makes the
/// motion flow instead of jerk at the corners.
fn jelly_stroke(x: f32) -> f32 {
    use std::f32::consts::PI;
    let peak = 0.3;
    if x < peak {
        0.5 - 0.5 * (PI * x / peak).cos() // 0 → 1 (quick contraction)
    } else {
        0.5 + 0.5 * (PI * (x - peak) / (1.0 - peak)).cos() // 1 → 0 (slower recovery)
    }
}

/// Build the graph a loaded `.gguf` model contributes to `neural_loaded`, for the
/// Mind view in `Shared.mind[2]` (#367 T1 / #507 T1 / #147 T3), as decoded by
/// [`math::mind_view_mode`].
///
/// - **0 (or anything else)** — `math::gguf_architecture_graph`: the specimen, from
///   the header alone. This is the default and it is unchanged from #367 Tier 1.
/// - **1** — `gguf_data::project_embedding_galaxy` + `math::embedding_galaxy_graph`:
///   the vocabulary embedding matrix, stride-sampled, dequantized, and projected to
///   3-D through a deterministic PCA basis. Nodes are lit by each token's **full**
///   N-D embedding norm — real geometry the 3-D shadow discards.
/// - **2** — the **Delta lens**: `lora::read_adapter_dir` over the adapter directory
///   named by `ipc::adapter_sidecar_path()`, folded to per-layer movement by
///   `math::delta_sites` and drawn by `math::delta_lens_graph`. The specimen's own
///   topology, shaped and lit by how far each site moved during a fine-tune.
///
/// (There is no Live view. The activation ring's per-frame overwrite happens at the
/// `topo == 5` seam and is gated to view **0** — which is what keeps the measured
/// training-time quantity and the proxy generation-time one off the same picture.)
///
/// On a galaxy **or** a Delta failure — no embedding tensor, an unsupported
/// quantization, a truncated file, no adapter chosen, a DoRA adapter — this returns
/// `None`, which **clears** the graph exactly like a failed header parse does.
/// Substituting the specimen would silently show the user a different thing than the
/// one they asked for; an empty scene plus a stderr line saying why is the honest
/// failure.
///
/// Note this is synchronous on the render thread: a galaxy read dequantizes ~20k rows
/// and an adapter read streams every `lora_A`/`lora_B` pair, so either will hitch once
/// when the view is selected. Both cache, so it is once per selection and not per
/// frame.
fn build_mind_graph(
    path: &str,
    h: &organon_core::gguf::GgufHeader,
    view: u32,
    extent: f32,
    cache: &mut Option<organon_core::gguf_data::GalaxyProjection>,
    delta: &mut Option<(String, math::DeltaSites)>,
) -> Option<math::NeuralGraph> {
    if view == 2 {
        // #147 T3. The adapter directory rides a sidecar, not `Shared` — a path is not
        // a control-rate value and `Shared` is append-only across a process boundary.
        let dir = std::fs::read_to_string(ipc::adapter_sidecar_path())
            .ok()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let dir = match dir {
            Some(d) => d,
            None => {
                eprintln!(
                    "mind: delta lens — no adapter selected ({} is missing or empty); \
                     clearing graph",
                    ipc::adapter_sidecar_path().display()
                );
                *delta = None;
                return None;
            }
        };
        if delta.as_ref().map(|(p, _)| p.as_str()) != Some(dir.as_str()) {
            let t0 = std::time::Instant::now();
            match organon_core::lora::read_adapter_dir(&dir) {
                Ok(summary) => {
                    let sites = math::delta_sites(&summary);
                    // Say what was measured, including what was NOT understood: an
                    // unrecognised module name still counts into its layer's backbone,
                    // but it lights no head or MLP node, and a silent version of that
                    // would look like a layer nobody trained.
                    let range = sites
                        .rms_range()
                        .map(|(lo, hi)| format!("{lo:.3e}…{hi:.3e} per weight"))
                        .unwrap_or_else(|| "nothing moved".to_string());
                    eprintln!(
                        "mind: delta lens — {} adapted modules over {} layers, RMS {range} \
                         ({} unclassified, {} layerless) from {dir} in {:.1}s",
                        summary.modules.len(),
                        sites.layers.len(),
                        sites.unclassified.len(),
                        sites.layerless.len(),
                        t0.elapsed().as_secs_f32()
                    );
                    if let Some(base) = summary.config.base_model_name_or_path.as_deref() {
                        // ⚠️ The delta is against whatever base the adapter was trained
                        // on, which may be a 4-bit quantization rather than the released
                        // weights. The file states the name and nothing more.
                        eprintln!("mind: delta lens — base as the adapter states it: {base}");
                    }
                    *delta = Some((dir.clone(), sites));
                }
                Err(e) => {
                    eprintln!("mind: delta lens unavailable for {dir} — clearing graph: {e}");
                    *delta = None;
                    return None;
                }
            }
        }
        let sites = match delta.as_ref() {
            Some((_, s)) => s,
            // Unreachable: the branch above either filled the cache or returned.
            None => return None,
        };
        return Some(math::delta_lens_graph(h.n_layers, h.n_heads, extent, sites));
    }
    if view == 1 {
        // Reuse the cached projection when we have one: rescaling to a new `extent` is
        // pure arithmetic, while re-projecting re-reads and dequantizes ~20k rows out of
        // the .gguf (seconds). Only a new model clears the cache.
        if let Some(p) = cache.as_ref() {
            return Some(math::embedding_galaxy_graph(&p.points, &p.norms, extent));
        }
        let t0 = std::time::Instant::now();
        return match organon_core::gguf_data::project_embedding_galaxy(
            std::path::Path::new(path),
            h,
            organon_core::gguf_data::GALAXY_MAX_POINTS,
        ) {
            Ok(p) => {
                let g = math::embedding_galaxy_graph(&p.points, &p.norms, extent);
                eprintln!(
                    "mind: embedding galaxy — {} (basis fitted on {} rows, {:.1}s)",
                    p.summary(),
                    p.basis_rows,
                    t0.elapsed().as_secs_f32()
                );
                *cache = Some(p);
                Some(g)
            }
            Err(e) => {
                eprintln!("mind: embedding galaxy unavailable for {path} — clearing graph: {e}");
                None
            }
        };
    }
    let g = math::gguf_architecture_graph(h, extent);
    eprintln!("mind: specimen → {} nodes, {} edges", g.nodes.len(), g.edges.len());
    Some(g)
}

/// #275 — an FNV-1a hash of the brain-model dials that affect geometry/wiring, so
/// the visual only rebuilds the (O(n²)-to-wire) brain graph when they actually change.
fn brain_cache_key(sample: usize, extent: f32, br: &[f32; 16]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |x: u32| {
        h ^= x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(sample as u32);
    mix(extent.to_bits());
    for v in &br[0..8] {
        mix(v.to_bits()); // [0..5] geometry (T1) + [5..8] white matter (T2)
    }
    h
}

fn pulse_envelope(s: &ipc::Shared, beat_pos: f64) -> f32 {
    if s.pulse_source == 1 {
        s.audio[1].clamp(0.0, 4.0)
    } else {
        (-(beat_pos.rem_euclid(1.0) as f32) * 6.0).exp()
    }
}

/// Audio-driven dipole drive amplitude (#248 Tier 1). Off → 1 (the undriven
/// field, byte-identical). On → `floor + amount·RMS` from the plugin's smoothed
/// broadband loudness envelope (`audio[5]`, zero when Audio Reactive is off or
/// the track is silent), clamped to a sane range. The drive scales the Maxwell
/// #391 Tier 1: reconstruct the active field for the quantitative instrumentation,
/// using the SAME parameter indices + clocks the aura/energy bake uses (so the probe
/// numbers agree with what's drawn). Returns `None` for non-field generators. The
/// per-beat acoustic pump is left out (drive = the loudness envelope only) so a probe
/// reads a steady field amplitude; that's the only intentional divergence from the
/// aura bake.
fn instrument_field(
    s: &ipc::Shared,
    gen_phase: f32,
    maxdip_phase: f32,
    beat_pos: f64,
) -> Option<math::AnalyticField> {
    match GeneratorMode::from_u32(s.generator) {
        GeneratorMode::MaxwellField => {
            let m = &s.maxwell;
            // The instrument reconstructs the field the visual ENERGIZES (aura/energy
            // bake), which blends E↔B by the *aura* dial `maxenergy[7]` (`mx_aura_blend`)
            // — NOT the generator's geometry blend `maxwell[1]` (`mx_gen_blend`). Using
            // the aura blend keeps the reconstructed field identical to the energized one
            // (blend only feeds `energy()`/`velocity()`; `probe()` reads raw E,B).
            let aura_blend = s.maxenergy[7];
            let band_elems = if audio_multipole_on(s) { audio_band_elems(s) } else { Vec::new() };
            if !band_elems.is_empty() {
                Some(math::AnalyticField::MaxwellBands {
                    elems: band_elems,
                    blend: aura_blend,
                    near: m[7],
                    r_min: m[10],
                    phase: maxdip_phase,
                })
            } else {
                let dipoles = m[3] > 0.5;
                let mut sources =
                    math::maxwell_sources((m[2] as usize).max(1), m[4], dipoles, m[5], m[6], gen_phase);
                let lean = audio_stereo_lean(s);
                if lean != 0.0 {
                    for src in &mut sources {
                        src.pos.x += lean;
                    }
                }
                let antenna_segs = if s.maxenergy[5] != 0.0 { 64 } else { 0 };
                Some(math::AnalyticField::Maxwell {
                    sources,
                    dipoles,
                    blend: aura_blend,
                    k: m[8],
                    near: m[7],
                    r_min: m[10],
                    phase: maxdip_phase,
                    antenna_len: s.maxenergy[4],
                    antenna_segs,
                    drive: audio_dipole_drive(s),
                    // Lean the finite antenna on X to match the drawn field (the point
                    // sources above are already leaned) — else the probe/ledger read the
                    // antenna centred at the origin while the visual draws it leaned.
                    offset: Vec3::new(lean, 0.0, 0.0),
                })
            }
        }
        GeneratorMode::Acoustic => {
            let a = &s.acoustic;
            let a2 = &s.acoustic2;
            if a2[0] > 0.5 {
                // Cavity standing-wave (Chladni) — mirror the aura's tweened 3-D mode walk.
                let a3 = &s.acoustic3;
                let base_modes = Vec3::new(a2[1], a2[2], a2[3]);
                let mut modes = math::cavity_morph_modes_tween(base_modes, beat_pos, a2[4], a3[0]);
                modes += Vec3::new(a3[1], a3[2], a3[3]) * cavity_audio_breathe(s);
                Some(math::AnalyticField::AcousticCavity {
                    modes,
                    dims: Vec3::splat(a2[5].max(1.0e-3)),
                    blend: a[14],
                    phase: maxdip_phase,
                    drive: audio_dipole_drive(s),
                    intensity: a2[6],
                })
            } else {
                let kind = math::AcousticKind::from_u32(a[0] as u32);
                let band_drives = audio_band_drives(s);
                let use_bands = audio_multipole_on(s) && band_drives.iter().any(|d| *d > 0.0);
                let mut sources = if use_bands {
                    math::acoustic_band_sources(&band_drives, a[4])
                } else {
                    math::acoustic_sources(kind, a[4])
                };
                let lean = if s.audiodip[0] != 0.0 {
                    s.audio[7].clamp(-1.0, 1.0) * s.audiodip[6].clamp(0.0, 1.0) * a[4].max(1.0)
                } else {
                    0.0
                };
                if lean != 0.0 {
                    for src in &mut sources {
                        src.pos.x += lean;
                    }
                }
                Some(math::AnalyticField::Acoustic {
                    sources,
                    blend: a[14],
                    k: a[1],
                    near: a[2],
                    r_min: a[5],
                    phase: maxdip_phase,
                    drive: if use_bands { 1.0 } else { audio_dipole_drive(s) },
                    intensity: a2[6],
                })
            }
        }
        _ => None,
    }
}

/// source's amplitude: E,B linearly, the rendered energy density quadratically.
fn audio_dipole_drive(s: &ipc::Shared) -> f32 {
    if s.audiodip[0] == 0.0 {
        return 1.0;
    }
    if calibrated_mode(s) {
        // #333 Tier 3: a reproducible law of the MEASURED momentary LUFS
        // (`audiometer[0]`) — floor at `ad_floor`, reaching `ad_amount` at the
        // loudness target — instead of the arbitrary gain·RMS below.
        let scaled = math::calibrated_drive(s.audiometer[0], s.analytical[2], s.analytical[1], s.audiodip[1]);
        (s.audiodip[2] + scaled).clamp(0.0, 4.0)
    } else {
        (s.audiodip[2] + s.audiodip[1] * s.audio[5].max(0.0)).clamp(0.0, 4.0)
    }
}

/// #333 Tier 3: is the Duo-Field in Calibrated mode (reproducible from measured LUFS)?
#[inline]
fn calibrated_mode(s: &ipc::Shared) -> bool {
    s.analytical[0] > 0.5
}

/// #333 Tier 3: aggregate the calibrated RTA (`audiospectrum`, dBFS) into the 5 coarse
/// bands by frequency (peak dB per band), for the calibrated per-band multipole drive.
/// Octave modes only — returns the −120 floor per band when the RTA header can't place
/// the bins (linear mode / no data), so the caller falls back cleanly.
fn calibrated_5band_db(s: &ipc::Shared) -> [f32; math::AUDIO_BANDS] {
    const EDGES: [f32; math::AUDIO_BANDS + 1] = [20.0, 60.0, 160.0, 500.0, 2000.0, 16000.0];
    let mut out = [-120.0f32; math::AUDIO_BANDS];
    let n = (s.audiometer[11] as usize).min(s.audiospectrum.len());
    let denom = s.audiometer[12]; // octave denominator (0 = linear FFT, unsupported here)
    let c0 = s.audiometer[13]; // band-0 centre Hz
    if n == 0 || denom <= 0.0 || c0 <= 0.0 {
        return out;
    }
    for i in 0..n {
        let fc = c0 * 2f32.powf(i as f32 / denom);
        for b in 0..math::AUDIO_BANDS {
            if fc >= EDGES[b] && fc < EDGES[b + 1] {
                out[b] = out[b].max(s.audiospectrum[i]);
            }
        }
    }
    out
}

/// Calibrated-colour LUT coordinate (#349 Tier 1): resolve the frame's single
/// representative measured level per the `Shared.colour` `source`, then map it to a
/// `0..1` LUT coord across the `[lo_db, hi_db]` window (`math::db_to_colour_t`).
///
/// Source:
///  - **Auto (0):** field generators (Maxwell id 8 / Acoustic id 23) → a BAND dBFS
///    level; every other generator → momentary LUFS (`audiometer[0]`).
///  - **Band (1):** always the band dBFS level.
///  - **Lufs (2):** always the momentary LUFS.
///
/// The representative band level (Tier 1) is the **peak** calibrated band in
/// `audiospectrum` (the loudest measured band this frame) — honest, cheap, and
/// generator-agnostic; true per-node/per-band spatial colouring is #348/#349 Tier 3.
/// Returns 0 when there's no data (silence floors to `lo_db → 0`, so the tint sits at
/// the bottom of the LUT — the calibrated `amount` still governs how much shows).
/// Momentary LUFS, treating the UNMEASURED default (exactly 0.0) as silence. The plugin
/// floors real silence at −120 and a true 0.0-dBFS momentary reading never occurs, so 0.0
/// means "no audio measured yet" — without this the calibrated colour/drive read FULL
/// SCALE (top of the LUT / max drive) before any audio arrives (e.g. standalone). (Bugbot)
fn momentary_lufs(s: &ipc::Shared) -> f32 {
    if s.audiometer[0] == 0.0 {
        -120.0
    } else {
        s.audiometer[0]
    }
}

fn calibrated_colour_t(s: &ipc::Shared) -> f32 {
    use organon_core::params::CalColourSource;
    let lo_db = s.colour[1];
    let hi_db = s.colour[2];
    let source = CalColourSource::from_u32(s.colour[4] as u32);
    // Peak calibrated band (dBFS) across the measured RTA; −120 (the meter floor)
    // when the header can't place bins or there's no data.
    let peak_band_db = || -> f32 {
        let n = (s.audiometer[11] as usize).min(s.audiospectrum.len());
        let mut peak = -120.0f32;
        for i in 0..n {
            peak = peak.max(s.audiospectrum[i]);
        }
        peak
    };
    let is_field = matches!(
        GeneratorMode::from_u32(s.generator),
        GeneratorMode::MaxwellField | GeneratorMode::Acoustic
    );
    let level_db = match source {
        CalColourSource::Band => peak_band_db(),
        CalColourSource::Lufs => momentary_lufs(s),
        CalColourSource::Auto => {
            if is_field {
                peak_band_db()
            } else {
                momentary_lufs(s)
            }
        }
    };
    math::db_to_colour_t(level_db, lo_db, hi_db)
}

/// Per-axis cavity **breathe** amount (#325 Tier 5): the broadband loudness that lifts
/// each cavity mode number, gated by the same "audio drives the source" toggle as the
/// dipole drive. 0 when the drive is off (byte-identical) or the track is silent — so
/// the per-axis gains are inert until both the drive is on and there's signal.
fn cavity_audio_breathe(s: &ipc::Shared) -> f32 {
    if s.audiodip[0] == 0.0 {
        return 0.0;
    }
    if calibrated_mode(s) {
        // #333 Tier 3: 0..1 reproducible from the measured LUFS (1.0 at the target).
        math::calibrated_drive(s.audiometer[0], s.analytical[2], s.analytical[1], 1.0)
    } else {
        s.audio[5].max(0.0)
    }
}

/// Spectrum → multipole mode gate (#248 Tier 2): needs the audio drive on AND
/// the multipole toggle. In this mode the five band envelopes (not the broadband
/// RMS) drive distinct multipole moments.
fn audio_multipole_on(s: &ipc::Shared) -> bool {
    s.audiodip[0] != 0.0 && s.audiodip[3] != 0.0
}

/// Per-band drive amplitudes (#248 Tier 2): the Tier-1 mapping applied per band
/// envelope — `drive_b = floor + amount·band_b`, clamped like the broadband
/// drive. Silence decays every band to the floor (a dim idle multipole of every
/// order); floor 0 lets silent bands vanish entirely (their elements are
/// skipped at build time).
fn audio_band_drives(s: &ipc::Shared) -> [f32; math::AUDIO_BANDS] {
    if calibrated_mode(s) {
        // #333 Tier 3: reproducible per-band drive from the calibrated RTA aggregated
        // into the 5 coarse bands (dBFS), floor −60 → 0, −6 dBFS → `ad_amount`. Falls
        // back to −120 (→ floor) per band when the RTA header can't be reconstructed.
        let band_db = calibrated_5band_db(s);
        std::array::from_fn(|b| {
            let scaled = math::calibrated_drive(band_db[b], -60.0, -6.0, s.audiodip[1]);
            (s.audiodip[2] + scaled).clamp(0.0, 4.0)
        })
    } else {
        std::array::from_fn(|b| {
            (s.audiodip[2] + s.audiodip[1] * s.audio[b].max(0.0)).clamp(0.0, 4.0)
        })
    }
}

/// Build the band-multipole element stack for the current frame (#248 Tier 2).
/// Reuses the Maxwell generator's dials: Separation = the array extent,
/// wavenumber k = the base wavelength, near/r_min = the field shaping; the
/// `ad_spread` dial compresses the honest per-band wavelength ratio.
fn audio_band_elems(s: &ipc::Shared) -> Vec<math::BandElem> {
    let m = &s.maxwell;
    let mut elems = math::maxwell_band_elements(
        &audio_band_drives(s),
        m[4],           // separation → multipole array extent
        s.audiodip[4],  // spread: per-band k compression
        m[8],           // base wavenumber
        m[7],           // near-field weight
        m[10],          // r_min clamp
    );
    // #248 Tier 3: the stereo lean shifts the whole multipole stack along X with the mix.
    let lean = audio_stereo_lean(s);
    if lean != 0.0 {
        for e in &mut elems {
            e.pos.x += lean;
        }
    }
    elems
}

/// Stereo lean (#248 Tier 3): the smoothed mix balance (`audio[7]`, −1..+1) shifts
/// the source along X by up to ±Separation·stereo — the field leans with the mix.
/// 0 when the drive or the stereo dial is off.
fn audio_stereo_lean(s: &ipc::Shared) -> f32 {
    if s.audiodip[0] == 0.0 {
        return 0.0;
    }
    s.audio[7].clamp(-1.0, 1.0) * s.audiodip[6].clamp(0.0, 1.0) * s.maxwell[4].max(1.0)
}

/// Continuous-mode winding velocity for one axis: `1 + depth · wave(phase)`,
/// clamped to stay forward (≥ 0) and sane (tan/log can spike). `depth = 0` → 1
/// (constant spin). For a bounded wave at `depth ≤ 1` the speed swings in [0, 2]
/// around a mean of 1, so the waveform changes the *character* of the spin within
/// each cycle without changing the overall rate (which the global Speed sets).
fn wind_velocity(func: FuncName, phase: f64, depth: f64) -> f64 {
    (1.0 + depth * math::apply_func(func, phase)).clamp(0.0, 8.0)
}

/// Advance the Speed-Pulse envelope and return the global-speed multiplier.
/// `drive` (the pulse level, clamped to 0..1) kicks `bounce` up with the `attack`
/// time constant; it falls back with `decay` (both seconds). The multiplier is
/// `10^(bounce·amount)`, so `amount` is in *decades* — a full hit at amount=1 is a
/// ×10 speed bounce that then decays. `amount = 0` (or `drive = 0`, e.g. pulse
/// off) settles the multiplier back to ~1, so it's inert by default.
fn speed_pulse_mult(
    bounce: &mut f64,
    drive: f32,
    amount: f64,
    attack_s: f64,
    decay_s: f64,
    dt: f64,
) -> f64 {
    let target = (drive as f64).clamp(0.0, 1.0);
    let tau = if target > *bounce { attack_s.max(1.0e-4) } else { decay_s.max(1.0e-4) };
    let coeff = 1.0 - (-dt / tau).exp();
    *bounce += (target - *bounce) * coeff;
    10f64.powf(bounce.clamp(0.0, 1.0) * amount)
}

/// Advance the Breath envelope and return the (uniform) scene scale vector.
/// `drive` (the pulse level, clamped 0..1) rises into `bounce` with the `attack`
/// time constant and falls with `decay` (both seconds). `g = amount · bounce` is
/// the scale depth, so a full pulse swells the whole scene by × (1 + amount).
/// `amount = 0` (or `drive = 0`, e.g. pulse off) settles back to `Vec3::ONE` —
/// inert by default.
fn breath_scale_vec(
    bounce: &mut f64,
    drive: f32,
    amount: f64,
    attack_s: f64,
    decay_s: f64,
    dt: f64,
) -> Vec3 {
    let target = (drive as f64).clamp(0.0, 1.0);
    let tau = if target > *bounce { attack_s.max(1.0e-4) } else { decay_s.max(1.0e-4) };
    let coeff = 1.0 - (-dt / tau).exp();
    *bounce += (target - *bounce) * coeff;

    let g = (amount * bounce.clamp(0.0, 1.0)).max(0.0);
    Vec3::splat((1.0 + g) as f32)
}

/// Advance `beat_pos` by `dt` seconds. Free-runs at the active BPM every frame
/// (host tempo when locked, else the manual slider), then — when tempo-sync is
/// on and the host is playing — gently corrects the *phase* toward the host's
/// `pos_beats`. This keeps motion smooth at any frame rate, survives a stopped
/// transport and standalone (no host), yet locks to the grid when one exists.
/// The BPM the beat clock free-runs at, per the camera clock's `tempo_source`
/// (#307): Host transport (today's default), a BPM detected from the audio, or the
/// manual dial. The Audio source holds the last detected BPM through a breakdown
/// (the estimator zeroes `cam_audio[0]` only before it has ever locked, so a
/// mid-set gap keeps the last value); it falls back to the manual dial until the
/// first lock. Shared by `advance_beat_clock` and the camera-momentum clock so both
/// keep the same time.
fn active_bpm(s: &ipc::Shared) -> f64 {
    // The single shared rule (also used by the plugin's synth beat clock + footer).
    // `tempo_sync` is intentionally not passed — it governs only the phase PLL-lock
    // in `advance_beat_clock`, not whether Host follows the host tempo.
    math::resolve_bpm(
        s.cam_clock[0] as u32,
        s.tempo as f64,
        s.transport[2] as f64,
        s.transport[3] > 0.5,
        s.cam_audio[0] as f64,
    )
}

/// Tempo-synced Maxwell dipole-oscillation phase (radians, wrapped to `[0, τ)`).
/// The field completes one full there-and-back per `div`'s beat length off the PLL
/// `beat_pos`; `beats_per_bar` scales the Bar / 2-Bar divisions to the session's
/// time signature. Wrapping keeps f32 precise over long sessions (the retarded
/// `cos(fphase − k·r)` is continuous across the whole-period wrap).
fn maxwell_osc_phase(beat_pos: f64, div: OscDivision, beats_per_bar: f32) -> f32 {
    let period = (div.beats(beats_per_bar) as f64).max(1.0e-3);
    (std::f64::consts::TAU * beat_pos / period).rem_euclid(std::f64::consts::TAU) as f32
}

/// The record-length options the 'B' key cycles through (#430): the one-shot bar counts
/// plus **Free** (0 = manual toggle — press R again to stop). Order wraps back to 8.
/// Cycle the #430 chunk-mode phrase length, in **beats** (Shift+B): 4 → 8 → 16 → 32 → 4.
/// Beats, not bars, because a music-video cut is a phrase — the motivating case is 8-beat
/// clips (two bars of 4/4). Powers of two also keep the grid continuous across the host's
/// `pos_beats` wrap at 1024.
fn next_phrase_beats(beats: f64) -> f64 {
    match beats.round() as i64 {
        4 => 8.0,
        8 => 16.0,
        16 => 32.0,
        32 => 4.0,
        _ => 8.0,
    }
}

fn next_record_bars(bars: u32) -> u32 {
    match bars {
        8 => 16,
        16 => 32,
        32 => 64,
        64 => 0, // Free (manual toggle)
        _ => 8,  // Free / anything → back to 8
    }
}

/// Human label for a record-length selection (`0` = Free / manual toggle).
fn record_len_label(bars: u32) -> String {
    if bars == 0 {
        "Free (press R to stop)".to_string()
    } else {
        format!("{bars} bars")
    }
}

fn advance_beat_clock(beat_pos: &mut f64, host_pos_prev: &mut f64, s: &ipc::Shared, dt: f64, fixed: bool) {
    let playing = s.transport[0] > 0.5;
    let host_pos = s.transport[1] as f64;
    let host_has_tempo = s.transport[3] > 0.5;
    // The host grid is only usable for phase-locking if `pos_beats` is actually
    // ADVANCING — many hosts don't hand it to an audio effect (it's stamped as a
    // frozen 0), and locking to that stalls the beat. So detect movement and only
    // lock when it's live (ignoring the mod-1024 wrap, a large negative jump).
    let delta = host_pos - *host_pos_prev;
    let host_advancing = playing && delta.abs() > 1.0e-6 && delta > -512.0;
    *host_pos_prev = host_pos;
    // Only the Host source PLL-locks its phase to the transport; Audio + Manual
    // free-run (there's no host grid to lock to — the point of those modes).
    let host_source = (s.cam_clock[0] as u32) == 0;
    // Perfect / fixed-timestep capture free-runs the clock: the wall clock isn't real time,
    // so PLL-locking to the host's (still real-time) transport would fight the fixed step.
    let locked = !fixed && host_source && s.tempo_sync != 0 && host_has_tempo && host_advancing;

    let bpm = active_bpm(s);
    *beat_pos += dt * bpm.max(0.0) / 60.0;

    if locked {
        // Shortest signed phase error in [-0.5, 0.5) beats.
        let mut err = (host_pos - *beat_pos).rem_euclid(1.0);
        if err > 0.5 {
            err -= 1.0;
        }
        // Exponential approach: fraction corrected this frame.
        let k = 1.0 - (-dt / PLL_TAU).exp();
        *beat_pos += err * k;
    }
}

/// Integrate the auto-orbit phase with beat-driven momentum. Each beat crossing
/// injects `kick` into the angular velocity, which then bleeds off (per-beat
/// retention `damping`, 0..1) — so the camera lurches on the beat and coasts
/// between. `base_speed` is a constant drift so it never fully stalls. All rates
/// are in beat-time; `dt_beats` is the elapsed beats this frame, `kicks` the
/// number of whole beats crossed. Phase is wrapped to [0,1) to stay bounded.
fn advance_camera(
    phase: &mut f64,
    vel: &mut f64,
    base_speed: f64,
    kick: f64,
    damping: f64,
    dt_beats: f64,
    kicks: f64,
) {
    *vel += kick * kicks;
    *phase = (*phase + (base_speed + *vel) * dt_beats).rem_euclid(1.0);
    *vel *= damping.clamp(0.0, 1.0).powf(dt_beats.max(0.0));
}

/// A full camera-move offset (#307): the yaw/pitch orbit swing, a radius
/// multiplier, plus the Tier-2 framing axes — a roll (dutch), a lateral truck
/// offset (fractions of the orbit radius, in the camera's right/up plane), and an
/// FOV multiplier (dolly-zoom). All are layered on top of the manual orbit;
/// `Default` is inert (identity).
#[derive(Clone, Copy)]
pub struct CamOffset {
    pub dyaw: f32,
    pub dpitch: f32,
    pub dist: f32,
    pub roll: f32,
    pub lat_x: f32,
    pub lat_y: f32,
    pub fov_mul: f32,
}

impl Default for CamOffset {
    fn default() -> Self {
        CamOffset {
            dyaw: 0.0,
            dpitch: 0.0,
            dist: 1.0,
            roll: 0.0,
            lat_x: 0.0,
            lat_y: 0.0,
            fov_mul: 1.0,
        }
    }
}

impl CamOffset {
    /// Linear crossfade A→B at `g` (0..1) — the glide blend between two shots.
    fn mix(a: CamOffset, b: CamOffset, g: f32) -> CamOffset {
        let l = |x: f32, y: f32| x + (y - x) * g;
        CamOffset {
            dyaw: l(a.dyaw, b.dyaw),
            dpitch: l(a.dpitch, b.dpitch),
            dist: l(a.dist, b.dist),
            roll: l(a.roll, b.roll),
            lat_x: l(a.lat_x, b.lat_x),
            lat_y: l(a.lat_y, b.lat_y),
            fov_mul: l(a.fov_mul, b.fov_mul),
        }
    }
}

/// Value-noise-ish smooth pseudo-random in [-1,1] from a continuous input, for the
/// handheld-drift move — a low-frequency wander with no beat sync. Deterministic
/// (a hashed lerp of integer lattice points), so it never jitters frame-to-frame.
fn drift_noise(x: f64, seed: u32) -> f64 {
    let hash = |i: i64| {
        let mut h = (i as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (seed as u64);
        h ^= h >> 30;
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 27;
        // Top 32 bits → [0, 2³²); /2³¹ → [0, 2); −1 → [−1, 1). (A /2³¹ of the top 31
        // bits — `h >> 33` — only spans [−1, 0), the #316 Bugbot one-sided-drift bias.)
        ((h >> 32) as f64 / (1u64 << 31) as f64) - 1.0 // [-1, 1)
    };
    let i = x.floor();
    let f = x - i;
    let u = f * f * (3.0 - 2.0 * f); // smoothstep fade
    let a = hash(i as i64);
    let b = hash(i as i64 + 1);
    a + (b - a) * u
}

/// Map an auto-orbit path id + phase (cycles) to a `CamOffset`, layered on top of
/// the manual orbit. `amount` (0..1) scales the swing. Path 0 = Off → identity.
fn camera_path_offset(path: u32, phase: f64, amount: f32) -> CamOffset {
    use std::f64::consts::TAU;
    let a = amount.clamp(0.0, 1.0) as f64;
    let p = phase * TAU;
    let mut o = CamOffset::default();
    match path {
        1 => {
            // Horizontal circle.
            o.dyaw = p as f32;
        }
        2 => {
            // Vertical sweep.
            o.dpitch = (p.sin() * 1.3 * a) as f32;
        }
        3 => {
            // Figure-eight.
            o.dyaw = (p.sin() * 1.2 * a) as f32;
            o.dpitch = ((2.0 * p).sin() * 0.7 * a) as f32;
        }
        4 => {
            // Spiral (orbit + rise + gentle zoom).
            o.dyaw = p as f32;
            o.dpitch = ((0.5 * p).sin() * 0.8 * a) as f32;
            o.dist = (1.0 + p.sin() * 0.25 * a) as f32;
        }
        5 => {
            // Boom / crane: a slow orbit while the eye rises + falls (pedestal).
            o.dyaw = (p * 0.5) as f32;
            o.dpitch = (p.sin() * 0.9 * a) as f32;
            o.dist = (1.0 + (1.0 - p.cos()) * 0.15 * a) as f32; // ease back as it lifts
        }
        6 => {
            // Pendulum: eased yaw swing back and forth (a designed to-and-fro, not a
            // constant turntable). sin gives the ease-in/ease-out at the ends.
            o.dyaw = (p.sin() * 1.6 * a) as f32;
            o.roll = (p.cos() * 8.0 * a).to_radians() as f32; // a little lean into the swing
        }
        7 => {
            // Truck: slide the framing laterally (and a touch vertically) — no orbit.
            o.lat_x = (p.sin() * 0.6 * a) as f32;
            o.lat_y = ((2.0 * p).sin() * 0.12 * a) as f32;
        }
        8 => {
            // Push / pull: a slow dolly ramp in and out along the view axis.
            o.dist = (1.0 - (0.5 * (1.0 - p.cos())) * 0.6 * a) as f32; // 1 → ~0.4 → 1
        }
        9 => {
            // Over the top: a true polar orbit that carries the eye up and over.
            o.dyaw = (p * 0.25) as f32;
            o.dpitch = (p.sin() * 1.45 * a) as f32; // nearly straight up at the peak
        }
        10 => {
            // Handheld drift: low-frequency noise wander on yaw/pitch/roll — organic,
            // not beat-locked.
            o.dyaw = (drift_noise(phase * 0.7, 1) * 0.5 * a) as f32;
            o.dpitch = (drift_noise(phase * 0.55, 2) * 0.35 * a) as f32;
            o.roll = (drift_noise(phase * 0.4, 3) * 6.0 * a).to_radians() as f32;
        }
        _ => {}
    }
    o
}

/// The camera moves the shot sequencer cycles through (#307): every non-Off
/// `CamPath` id — Tier 1's four orbits + Tier 2's cinematic moves (Boom, Pendulum,
/// Truck, Push/Pull, Over-the-Top, Drift).
const SEQ_PATHS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

/// One xorshift step — deterministic, no `Math.random`, seeded per-run. Advances
/// `state` in place and returns it.
fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    if x == 0 {
        x = 0x9e37_79b9; // never latch at zero
    }
    *state = x;
    x
}

/// Pick the next move id from `SEQ_PATHS` given the order (#307). Series cycles in
/// list order; Random picks a different id (no immediate repeat); Weighted is a
/// no-immediate-repeat random biased toward the lower ids (the hero orbits) — it
/// draws two candidates and keeps the earlier one. Shuffle (order 2) is handled
/// statefully in `SeqState` (a bag), so it falls through to Random here.
fn next_shot(cur: u32, order: u32, rng: &mut u32) -> u32 {
    let paths = SEQ_PATHS;
    if paths.is_empty() {
        return 0;
    }
    if paths.len() == 1 {
        return paths[0];
    }
    match order {
        0 => {
            // Series.
            let idx = paths.iter().position(|&p| p == cur).unwrap_or(0);
            paths[(idx + 1) % paths.len()]
        }
        3 => {
            // Weighted: two draws, keep the lower id (favours the classic orbits),
            // never an immediate repeat.
            loop {
                let r1 = paths[(xorshift(rng) as usize) % paths.len()];
                let r2 = paths[(xorshift(rng) as usize) % paths.len()];
                let pick = r1.min(r2);
                if pick != cur {
                    return pick;
                }
            }
        }
        _ => {
            // Random (and Shuffle's fallback): different id than the current.
            loop {
                let r = (xorshift(rng) as usize) % paths.len();
                if paths[r] != cur {
                    return paths[r];
                }
            }
        }
    }
}

/// Shot-sequencer state (#307): the current + outgoing move ids, the bar block last
/// seen, the bar position where the last change happened (glide origin), and the
/// RNG. `step` advances it on each `bars_per_shot`-bar boundary; `glide_t` reports
/// the crossfade progress since the last change.
pub struct SeqState {
    pub cur: u32,
    pub prev: u32,
    last_block: i64,
    boundary_bar: f64,
    rng: u32,
    /// Shuffle bag: a draining permutation of `SEQ_PATHS`; refilled when empty so no
    /// move repeats until all have played (#307 Tier 2).
    bag: Vec<u32>,
    /// Set true on the frame a shot actually changed — lets the caller reset the
    /// move phase for phrase-locked facing.
    pub just_changed: bool,
}

impl SeqState {
    pub fn new() -> SeqState {
        SeqState {
            cur: SEQ_PATHS[0],
            prev: SEQ_PATHS[0],
            last_block: i64::MIN,
            boundary_bar: 0.0,
            rng: 0x1234_5678,
            bag: Vec::new(),
            just_changed: false,
        }
    }

    /// Draw the next shuffle-bag id (Fisher–Yates by draining), refilling + avoiding
    /// an immediate repeat across bag boundaries.
    fn shuffle_next(&mut self) -> u32 {
        if self.bag.is_empty() {
            self.bag.extend_from_slice(SEQ_PATHS);
        }
        let i = (xorshift(&mut self.rng) as usize) % self.bag.len();
        let pick = self.bag.swap_remove(i);
        if pick == self.cur && !self.bag.is_empty() {
            // Avoid an immediate repeat on a fresh bag: swap for another and put the
            // repeat back.
            let j = (xorshift(&mut self.rng) as usize) % self.bag.len();
            let other = self.bag.swap_remove(j);
            self.bag.push(pick);
            return other;
        }
        pick
    }

    /// Advance on bar boundaries. `bar_pos` is beats/beats-per-bar; a shot holds for
    /// `bars_per_shot` bars. On the first observation it just latches the block (no
    /// change from nowhere); after that each new block promotes `cur`→`prev` and
    /// picks a fresh `cur` — unless a `hold_prob` roll keeps the current shot.
    /// `hold01` is a 0..1 random draw supplied by the caller (so the decision is
    /// testable/deterministic); pass 1.0 to disable holding.
    pub fn step(&mut self, bar_pos: f64, bars_per_shot: f64, order: u32, hold_prob: f32) {
        let bps = bars_per_shot.max(1.0);
        let block = (bar_pos / bps).floor() as i64;
        self.just_changed = false;
        if block != self.last_block {
            let mut held = false;
            if self.last_block != i64::MIN {
                // Hold roll: sometimes keep the current shot for another period.
                let hold = hold_prob > 0.0 && {
                    let r = (xorshift(&mut self.rng) as f64) / (u32::MAX as f64);
                    (r as f32) < hold_prob
                };
                if hold {
                    held = true;
                } else {
                    self.prev = self.cur;
                    self.cur = if order == 2 {
                        self.shuffle_next()
                    } else {
                        next_shot(self.cur, order, &mut self.rng)
                    };
                    self.just_changed = true;
                }
            }
            self.last_block = block;
            // Only move the glide origin when the shot actually changed (or on the
            // first latch). A held shot keeps its boundary so `glide_t` stays
            // saturated at 1 — otherwise the Glide blend would snap back to `prev`
            // and re-crossfade to an unchanged `cur` on every held boundary.
            if !held {
                self.boundary_bar = block as f64 * bps;
            }
        }
    }

    /// Crossfade progress (0..1) since the last shot change. `trans_bars <= 0`
    /// (a Cut) returns 1 immediately (no blend).
    pub fn glide_t(&self, bar_pos: f64, trans_bars: f64) -> f32 {
        if trans_bars <= 1.0e-6 {
            return 1.0;
        }
        (((bar_pos - self.boundary_bar) / trans_bars) as f32).clamp(0.0, 1.0)
    }
}

/// The decoupled dolly radius multiplier (#307): a mean-centred in/out breath of
/// fractional `depth` over `period_bars`, on its own bar clock — independent of the
/// orbit speed. `depth <= 0` → 1.0 (inert). Waves: 0 Sine, 1 Triangle, 2 Ease
/// (dwells at the near + far points).
fn dolly_factor(period_bars: f64, depth: f32, wave: u32, bar_pos: f64) -> f32 {
    if depth <= 0.0 {
        return 1.0;
    }
    let period = period_bars.max(1.0e-3);
    let ph = (bar_pos / period).rem_euclid(1.0);
    let w = match wave {
        1 => 1.0 - 4.0 * (ph - 0.5).abs(),                     // triangle, −1..1
        2 => (2.0 * (std::f64::consts::TAU * ph).sin()).tanh() / (2.0f64).tanh(), // ease/dwell
        _ => (std::f64::consts::TAU * ph).sin(),               // sine
    };
    (1.0 + depth as f64 * w) as f32
}

// --- #307 Tier 3: storyboard ---

/// A storyboard shot slot lives at `cam_story[8 + slot*4] = [path, bars, radius, _]`.
fn story_shot_path(story: &[f32; 24], slot: usize) -> u32 {
    story.get(8 + slot * 4).copied().unwrap_or(0.0) as u32
}
fn story_shot_bars(story: &[f32; 24], slot: usize) -> f64 {
    (story.get(8 + slot * 4 + 1).copied().unwrap_or(8.0) as f64).max(1.0)
}
fn story_shot_radius(story: &[f32; 24], slot: usize) -> f32 {
    let r = story.get(8 + slot * 4 + 2).copied().unwrap_or(1.0);
    if r > 0.01 { r } else { 1.0 }
}

/// Seed the storyboard RNG from the (integer) seed param, deterministically and
/// never zero — so a Random/Shuffle/Weighted storyboard replays identically.
fn seed_rng(seed: i32) -> u32 {
    let r = (seed as u32).wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
    if r == 0 {
        1
    } else {
        r
    }
}

/// Pick the next storyboard slot index (0..count) given the order (#307 Tier 3).
/// Series cycles; Random/Weighted avoid an immediate repeat (Weighted biases toward
/// earlier shots — intro-heavy — by keeping the lower of two draws). Shuffle is
/// handled statefully in `StoryState`, so it falls through to Random here.
fn next_slot(cur: usize, count: usize, order: u32, rng: &mut u32) -> usize {
    let n = count.max(1);
    if n == 1 {
        return 0;
    }
    match order {
        0 => (cur + 1) % n, // Series
        3 => loop {
            // Weighted.
            let r1 = (xorshift(rng) as usize) % n;
            let r2 = (xorshift(rng) as usize) % n;
            let pick = r1.min(r2);
            if pick != cur {
                return pick;
            }
        },
        _ => loop {
            // Random (+ Shuffle fallback).
            let r = (xorshift(rng) as usize) % n;
            if r != cur {
                return r;
            }
        },
    }
}

/// Storyboard playback state (#307 Tier 3): the active + outgoing slot, when the
/// current shot started (glide origin), the seeded RNG + shuffle bag, and the
/// manual "next shot" trigger bookkeeping. Each slot holds for its own `bars`.
pub struct StoryState {
    pub cur: usize,
    pub prev: usize,
    boundary_bar: f64,
    rng: u32,
    bag: Vec<usize>,
    last_bar_int: i64,
    pending_next: bool,
    last_next_gen: u32,
    started: bool,
    last_seed: i32,
    pub just_changed: bool,
}

impl StoryState {
    pub fn new() -> StoryState {
        StoryState {
            cur: 0,
            prev: 0,
            boundary_bar: 0.0,
            rng: seed_rng(1),
            bag: Vec::new(),
            last_bar_int: i64::MIN,
            pending_next: false,
            last_next_gen: 0,
            started: false,
            last_seed: i32::MIN,
            just_changed: false,
        }
    }

    fn shuffle_next(&mut self, count: usize) -> usize {
        let n = count.max(1);
        self.bag.retain(|&x| x < n);
        if self.bag.is_empty() {
            self.bag.extend(0..n);
        }
        let i = (xorshift(&mut self.rng) as usize) % self.bag.len();
        let pick = self.bag.swap_remove(i);
        if pick == self.cur && !self.bag.is_empty() {
            let j = (xorshift(&mut self.rng) as usize) % self.bag.len();
            let other = self.bag.swap_remove(j);
            self.bag.push(pick);
            return other;
        }
        pick
    }

    fn advance(&mut self, bar_pos: f64, count: usize, mode: u32) {
        self.prev = self.cur;
        self.cur = if mode == 2 {
            self.shuffle_next(count)
        } else {
            next_slot(self.cur, count, mode, &mut self.rng)
        };
        self.boundary_bar = bar_pos.floor(); // snap the glide origin to the bar grid
        // Only flag a real change (#318 Bugbot): a single-shot storyboard (count == 1)
        // re-picks slot 0 every boundary; flagging it would repeatedly reset
        // phrase-locked facing (cam_phase/cam_vel) and hitch the camera.
        self.just_changed = self.cur != self.prev;
    }

    /// Advance the storyboard from the `cam_story` block. Re-seeds on a seed change;
    /// advances a shot when its `bars` elapse, or on the next bar after a manual
    /// "next shot" trigger (`cam_story[4]` bumped).
    pub fn step(&mut self, bar_pos: f64, story: &[f32; 24]) {
        self.just_changed = false;
        let count = (story[1] as i64).clamp(1, 4) as usize;
        let mode = story[2] as u32;
        let seed = story[3] as i32;
        let next_gen = story[4] as u32;

        // (Re)initialize on first run or a seed change → reproducible playback.
        if !self.started || seed != self.last_seed {
            self.rng = seed_rng(seed);
            self.cur = 0;
            self.prev = 0;
            self.bag.clear();
            self.boundary_bar = bar_pos.floor();
            self.last_bar_int = bar_pos.floor() as i64;
            self.last_next_gen = next_gen;
            self.last_seed = seed;
            self.started = true;
            // Clear any armed "next shot" (#318 Bugbot): a trigger from the old
            // playlist must not fire immediately after a re-seed and skip the hold.
            self.pending_next = false;
            return;
        }
        // Clamp BOTH indices when the shot count shrinks (#318 Bugbot): leaving `prev`
        // on a now-removed slot would crossfade a shot that's no longer in the
        // playlist for one glide window.
        if self.cur >= count {
            self.cur = 0;
        }
        if self.prev >= count {
            self.prev = self.cur;
        }
        // Backward bar clock (#318 Bugbot): a host transport seek or PLL snap can drop
        // `bar_pos` below `boundary_bar`, which would leave `bar_pos - boundary_bar`
        // negative and stall scheduled advances until playback caught back up. Re-anchor
        // the glide origin so the current shot's timer restarts from the new position.
        if bar_pos + 1.0e-6 < self.boundary_bar {
            self.boundary_bar = bar_pos.floor();
            self.last_bar_int = bar_pos.floor() as i64;
        }

        // Manual "next shot": arm on the trigger edge, fire on the next bar (quantized).
        if next_gen != self.last_next_gen {
            self.pending_next = true;
            self.last_next_gen = next_gen;
        }
        let bi = bar_pos.floor() as i64;
        if bi != self.last_bar_int {
            self.last_bar_int = bi;
            if self.pending_next {
                self.pending_next = false;
                self.advance(bar_pos, count, mode);
                return;
            }
        }
        // Scheduled advance once the current shot's bars have elapsed.
        let cur_bars = story_shot_bars(story, self.cur);
        if bar_pos - self.boundary_bar >= cur_bars - 1.0e-6 {
            // A timed change also satisfies any armed manual "next" (#318 Bugbot):
            // otherwise `pending_next` (armed mid-bar) would fire a SECOND advance on
            // the next bar line — two shot skips from one button press.
            self.pending_next = false;
            self.advance(bar_pos, count, mode);
        }
    }

    /// Crossfade progress (0..1) since the last shot change (Cut = instant).
    pub fn glide_t(&self, bar_pos: f64, trans_bars: f64) -> f32 {
        if trans_bars <= 1.0e-6 {
            return 1.0;
        }
        (((bar_pos - self.boundary_bar) / trans_bars) as f32).clamp(0.0, 1.0)
    }
}

/// Full-depth modulation amount for a routing target (value added at env=1,
/// depth=1). Sized per target so one bipolar depth slider is musical across the
/// very different native ranges. Must match `ModTarget`'s discriminants.
fn mod_span(target: u32) -> f32 {
    match target {
        1 => 0.3,             // Scale Amp (0..0.5)
        2 => 1.5,             // Glow (0..2)
        3 | 4 | 5 => 60.0,    // Rotation Amp X/Y/Z (deg)
        6 | 7 | 8 => 1.0,     // Rotation Speed X/Y/Z (×inc_scale)
        9 | 10 | 11 | 14 => 80.0, // Translation Mod X/Y/Z (14 = all three)
        12 => 2.0,            // Exposure (EV stops)
        13 => 0.4,            // Bloom
        15 => 6.0,            // Rail Speed (units/beat — the Z0NE "gems" pump)
        _ => 0.0,             // None / unknown
    }
}

/// Add `signal · span(target)` to a routing target. Geometry targets land on
/// `pv` (fed to `draw_tissue`); look targets land on the local `s` copy (fed to
/// `build_uniforms`). Amounts that would drive a non-negative control below zero
/// are clamped.
fn apply_mod(s: &mut ipc::Shared, pv: &mut ParamValues, target: u32, signal: f32) {
    let delta = signal * mod_span(target);
    if delta == 0.0 {
        return;
    }
    match target {
        1 => pv.scale_amp = (pv.scale_amp + delta).max(0.0),
        2 => s.lighting[5] = (s.lighting[5] + delta).max(0.0), // glow
        3 => pv.rot_amp.x += delta,
        4 => pv.rot_amp.y += delta,
        5 => pv.rot_amp.z += delta,
        // Rotation Speed X/Y/Z — pump the per-axis speed the angle clock reads.
        6 => s.rot_mod[0] += delta,
        7 => s.rot_mod[1] += delta,
        8 => s.rot_mod[2] += delta,
        9 => pv.trans_mod.x += delta,
        10 => pv.trans_mod.y += delta,
        11 => pv.trans_mod.z += delta,
        12 => s.pbr[2] += delta,                               // exposure (EV; may go negative)
        13 => s.pbr[5] = (s.pbr[5] + delta).max(0.0),          // bloom
        14 => {                                                 // all three translation axes
            pv.trans_mod.x += delta;
            pv.trans_mod.y += delta;
            pv.trans_mod.z += delta;
        }
        // Rail Speed (#187): pump the rails generator's units-per-beat. Space
        // stretches with the envelope while beat alignment holds by construction
        // (the rail coordinate stays the beat clock). Floored above zero so a
        // deep negative depth can't reverse the ride.
        15 => s.rails[0] = (s.rails[0] + delta).max(0.05),
        _ => {}
    }
}

/// Rails slots that are LIVE — never latched by the quantized transition
/// (#187 Tier 3): speed [0] (the RailSpeed pulse pump acts now), horizon [8] +
/// fade [15] (window/perf dials), rib gain [9].
const RAILS_LIVE_SLOTS: [usize; 4] = [0, 8, 9, 15];

/// The rails quantized-transition latch (#187 Tier 3). Adopt `live` wholesale
/// on (re)entry (`fresh`); pass the LIVE slots through every frame; when the
/// geometry slots change, latch the block as PENDING with the next
/// change-every boundary (fixed by the FIRST change, so later tweaks land on
/// the same bar); a revert before the bar cancels; crossing the boundary
/// promotes pending to active.
/// Generate one scenery world's window into `out` from a 40-slot combined block
/// (rails timing/shape [0..24] ++ Terra landform [24..40], #206). Dispatches to
/// the Zone corridor (`rails_strands`) or the Terra landscape (`terra_strands`)
/// by `is_terra`; returns the topology + the membrane loft grid dims.
#[allow(clippy::too_many_arguments)]
fn gen_scenery_world(
    blk: &[f32; 40],
    is_terra: bool,
    u_now: f64,
    span: (f64, f64),
    beat_frac: f32,
    gen_phase: f32,
    palette: u32,
    color_phase: f32,
    out: &mut math::Strands,
) -> (math::Topology, (usize, usize)) {
    let spec = math::RailsSpec::from_slots(&blk[..24]);
    if is_terra {
        let terra = math::TerraSpec::from_slots(&blk[24..]);
        let topo =
            math::terra_strands(&spec, &terra, u_now, span, beat_frac, gen_phase, palette, color_phase, out);
        (topo, (spec.ring_n.max(3), 1))
    } else {
        let topo =
            math::rails_strands(&spec, u_now, span, beat_frac, gen_phase, palette, color_phase, out);
        (topo, math::rails_loft_dims(&spec))
    }
}

/// Scenery membrane loft (#206 Tier 1): skin one strand set into the scenery
/// membrane, APPENDING its vertex range past whatever's already there (so the
/// two-worlds transition can skin both the active and pending sides). `dims` is
/// the loft grid `(gx, gy)` — `rails_loft_dims` for Zone, `(lateral_n, 1)` for
/// Terra. Returns whether it lofted — a non-Grid strand set (Zone's Gates) has
/// no loft, so it returns false and the caller lowers to swept tubes instead.
#[allow(clippy::too_many_arguments)]
fn loft_scenery_append(
    strands: &[math::Strand],
    dims: (usize, usize),
    topo: math::Topology,
    palette: u32,
    color_phase: f32,
    pos: &mut Vec<Vec3>,
    norm: &mut Vec<Vec3>,
    col: &mut Vec<Vec4>,
    idx: &mut Vec<u32>,
) -> bool {
    if topo != math::Topology::Grid {
        return false;
    }
    let mem = math::strands_to_mem(strands);
    let (gx, gy) = dims;
    let (mut tp, mut tn, mut tc, mut ti) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    math::loft_membrane(&mem, gx, gy, 1, true, 0, false, palette, color_phase, &mut tp, &mut tn, &mut tc, &mut ti);
    let base = pos.len() as u32;
    pos.extend_from_slice(&tp);
    norm.extend_from_slice(&tn);
    col.extend_from_slice(&tc);
    idx.extend(ti.iter().map(|i| i + base));
    true
}

/// The quantized-transition latch (#187 Tier 3), generic over the block width
/// so it serves Zone's 24-slot rails block and Terra's 40-slot combined
/// (rails ++ terra) block alike. `RAILS_LIVE_SLOTS` (speed/horizon/ribs/fade)
/// and the `change_every` slot [3] index the rails half, valid for any N ≥ 24.
fn rails_latch_step<const N: usize>(
    active: &mut [f32; N],
    pending: &mut Option<([f32; N], f64)>,
    live: &[f32; N],
    u_now: f64,
    fresh: bool,
) {
    if fresh {
        *active = *live;
        *pending = None;
        return;
    }
    for i in RAILS_LIVE_SLOTS {
        active[i] = live[i];
    }
    let geo_eq = |a: &[f32; N], b: &[f32; N]| {
        a.iter()
            .zip(b.iter())
            .enumerate()
            .all(|(i, (x, y))| RAILS_LIVE_SLOTS.contains(&i) || x == y)
    };
    if geo_eq(live, active) {
        *pending = None; // matches the current world (incl. a pre-bar revert)
    } else {
        let ce = math::RAILS_CHANGE_TAB
            [(live[3] as usize).min(math::RAILS_CHANGE_TAB.len() - 1)] as f64;
        // ceil, not floor+ce: a change landing exactly ON a boundary (a Key
        // Map hit fired on the bar) applies to THAT downbeat via the promote
        // check below, not a full phrase later.
        let boundary = pending
            .map(|(_, b)| b)
            .unwrap_or_else(|| (u_now / ce).ceil() * ce);
        *pending = Some((*live, boundary));
    }
    if let Some((p, b)) = *pending {
        if u_now >= b {
            *active = p;
            for i in RAILS_LIVE_SLOTS {
                active[i] = live[i];
            }
            *pending = None;
        }
    }
}

/// Append-and-drain (#317 T1, finding #3): given the whole chat-sidecar body and how many
/// non-empty lines were already enqueued, return the messages appended SINCE the cursor +
/// the new cursor (the total non-empty line count). Blank lines are ignored on both sides
/// so a trailing newline never shifts the count. Every message survives rapid sends; none
/// is replayed once consumed.
fn agent_chat_drain(body: &str, consumed: usize) -> (Vec<String>, usize) {
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = consumed.min(lines.len());
    let pending = lines[start..]
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    (pending, lines.len())
}

/// #425: name one saved preset off the render thread. A detached worker owns the blocking
/// HTTP and writes the answer to this request's OWN reply file (`name_reply_path(id)`), so
/// two namings that finish close together can't clobber each other. It ALWAYS writes a reply
/// (empty on failure) so the editor consumes the pending entry rather than leaving the preset
/// stuck on its provisional name.
fn spawn_namer(req: agent::NameRequest) {
    std::thread::spawn(move || {
        let cfg = agent::AgentConfig::load();
        let client = agent::HttpChatClient;
        let name = agent::run_naming(&client, &cfg, &req).unwrap_or_default();
        if !name.is_empty() {
            mind_log::append(mind_log::MindEvent::Action, "namer", &name);
        }
        let _ = std::fs::write(ipc::name_reply_path(req.id), format!("{name}\n"));
    });
}

/// #425: service the pending preset-name request, if any. Reads + parses the request
/// sidecar, deletes it (so a later visual restart never re-runs an already-serviced name),
/// then spawns the namer. Called on each `name_gen` edge AND once at startup — the startup
/// call is what lets an in-flight save survive a visual restart, since the editor kept
/// running and still holds the matching pending entry.
fn service_name_request() {
    let Ok(body) = std::fs::read_to_string(ipc::name_request_path()) else {
        return;
    };
    let Ok(req) = serde_json::from_str::<agent::NameRequest>(&body) else {
        return;
    };
    let _ = std::fs::remove_file(ipc::name_request_path());
    spawn_namer(req);
}

/// #317 UI-sync: append one dispatched action's apply lines to the agent apply channel
/// (`organic-math-agent-apply.txt`), so the plugin editor mirrors it onto the real params
/// (sliders / dropdowns). Best-effort — the visual's own lane already renders the change, so
/// a failed append only costs the slider sync, never the visual. No-op for non-actuating
/// actions (read_state / describe / presets produce no ops).
fn append_agent_apply(action: &agent::AgentAction) {
    let ops = agent::apply_ops(action);
    if ops.is_empty() {
        return;
    }
    let body: String = ops.iter().map(|o| format!("{}\n", o.to_line())).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ipc::agent_apply_path())
    {
        use std::io::Write;
        let _ = f.write_all(body.as_bytes());
    }
}

/// #452 Tier 3: append one reply line to the eyes channel (visual → CLI), so a
/// `snap`/`record` invocation waiting on its nonce gets the path or the error.
fn append_eyes_reply(nonce: &str, result: &Result<String, String>) {
    let line = format!("{}\n", organon_core::eyes::eyes_reply_line(nonce, result));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ipc::eyes_reply_path())
    {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
    }
}

/// #452: append one raw line to the apply channel (the CLI drain's `release`,
/// which has no `AgentAction` — `apply_ops` never emits it).
fn append_agent_apply_line(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ipc::agent_apply_path())
    {
        use std::io::Write;
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- organon#217 T5: converge on hold ---------------------------------
    //
    // The raster → path-trace handover and the accumulation restart are pure functions
    // of (the preset's toggle, the ring's live / generation / settled), so the decision
    // is pinned here without a GPU. Each assertion names the mutation it guards: dropping
    // `generation` from the key, keying on a per-tick value, path-tracing a silent ring,
    // or touching a preset that already traces.

    fn ring(live: bool, generation: u32, settled: bool) -> GlyphPtState {
        GlyphPtState { live, generation, settled }
    }

    #[test]
    fn no_ring_is_the_pre_t5_gate() {
        let none = GlyphPtState::default();
        assert!(!none.live && none.generation == 0 && !none.settled);
        assert!(!pathtrace_active(false, none), "no ring, preset rasters → raster");
        assert!(pathtrace_active(true, none), "no ring, preset traces → trace");
        let s = ipc::Shared::default();
        assert_eq!(pt_content_key(&s, none), pt_content_key(&s, none));
    }

    #[test]
    fn a_glyph_frame_that_changed_restarts_and_a_held_one_accumulates() {
        let s = ipc::Shared::default();
        assert_ne!(
            pt_content_key(&s, ring(true, 7, false)),
            pt_content_key(&s, ring(true, 8, false)),
            "generation 7 → 8 (the glyphs moved) must change the content key, or accumulation never restarts on motion"
        );
        // The dwell: the producer republishes the held grid every heartbeat with the SAME
        // generation (it bumps on payload change, not per tick).
        let held = ring(true, 8, true);
        assert_eq!(
            pt_content_key(&s, held),
            pt_content_key(&s, held),
            "a heartbeat republish keeps the key, so the dwell accumulates"
        );
        assert_ne!(
            pt_content_key(&s, held),
            pt_content_key(&s, GlyphPtState::default()),
            "ring → silence → generator is a content change"
        );
        // Settling is a flag on the same payload, not a new payload: the key ignores it.
        assert_eq!(pt_content_key(&s, ring(true, 3, false)), pt_content_key(&s, ring(true, 3, true)));
        // And the tracer's own settings still key it (the pre-T5 half is intact).
        let mut glass = ipc::Shared::default();
        glass.ptglass[0] = 1.0;
        assert_ne!(pt_content_key(&s, held), pt_content_key(&glass, held));
    }

    #[test]
    fn the_tracer_runs_on_a_held_frame_and_never_on_motion_or_silence() {
        assert!(pathtrace_active(false, ring(true, 5, true)), "a live, settled ring hands the frame to the tracer");
        assert!(!pathtrace_active(false, ring(true, 5, false)), "motion rasters");
        assert!(
            !pathtrace_active(false, ring(false, 5, true)),
            "a settled flag on a ring that is not drawing (silence) must not be path-traced"
        );
        assert!(!pathtrace_active(false, ring(false, 0, false)));
    }

    #[test]
    fn a_preset_that_already_traces_is_untouched_by_the_ring() {
        for live in [false, true] {
            for settled in [false, true] {
                for g in [0, 1, 99] {
                    assert!(pathtrace_active(true, ring(live, g, settled)), "live={live} g={g} settled={settled}");
                }
            }
        }
    }

    #[test]
    fn the_restart_holds_the_count_at_zero_while_the_tracer_is_off() {
        assert!(pathtrace_restarts(false, false, false, false), "tracer off (raster during motion) → count held at 0");
        assert!(!pathtrace_restarts(false, false, false, true), "tracer on, nothing moved → accumulate");
        assert!(pathtrace_restarts(true, false, false, true), "camera moved");
        assert!(pathtrace_restarts(false, true, false, true), "buffers resized");
        assert!(pathtrace_restarts(false, false, true, true), "content changed");
    }

    #[test]
    fn the_dwell_converges_and_the_next_effect_restarts_it() {
        // One cycle as the world sees it, frame by frame, with the camera still and the
        // preset rastering: motion (generation climbing, not settled) → settle → dwell
        // (heartbeats at one generation) → the next effect. Mirrors `frame_body`'s order:
        // restart decided first, this frame's sample index is `spp`, then the count
        // advances iff a trace was issued.
        let s = ipc::Shared::default();
        let frames = [
            ring(true, 1, false),
            ring(true, 2, false),
            ring(true, 3, false),
            ring(true, 3, true),
            ring(true, 3, true),
            ring(true, 3, true),
            ring(true, 4, false),
            ring(true, 5, false),
            GlyphPtState::default(), // the producer went silent: back to the generator
        ];
        let mut prev = PT_CONTENT_NONE;
        let mut spp = 0u32;
        let mut trace = Vec::new();
        for f in frames {
            let key = pt_content_key(&s, f);
            let active = pathtrace_active(false, f);
            if pathtrace_restarts(false, false, key != prev, active) {
                spp = 0;
            }
            prev = key;
            trace.push((active, spp));
            if active {
                spp += 1;
            }
        }
        assert_eq!(
            trace,
            vec![
                (false, 0),
                (false, 0),
                (false, 0),
                (true, 0), // first traced frame of the dwell starts from a clean buffer
                (true, 1),
                (true, 2), // converging: 3 spp accumulated by the end of the hold
                (false, 0), // the next effect: raster again, count dropped
                (false, 0),
                (false, 0),
            ]
        );
    }

    // ---- #541 S2 T3: the world/window seam -------------------------------
    //
    // The GPU half of the offscreen path can't run here (no GPU, no display) —
    // what IS checkable, and is where the refactor could go subtly wrong, is the
    // *decision* every frame makes about its destination: which size/format it
    // draws at, whether the window-only side effects apply, and what an offscreen
    // target does about HDR headroom + wide gamut.

    const SDR: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
    const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

    // ---- #582: an offscreen pass must not be able to end a recording ------
    //
    // This is the bug that killed every take after 2–3 frames once an editor was open: the
    // frame mirror renders a 640×360 offscreen pass through the same frame body, it disagreed
    // with the take's latched 1100×760, and the auto-stop believed it. Pure predicate, so the
    // one thing no GPU-less environment could check before is now checked here.

    #[test]
    fn an_offscreen_frame_never_ends_a_take() {
        // Every combination that WOULD stop a presented frame must be inert when not presented.
        for done in [false, true] {
            for matches in [false, true] {
                assert!(
                    !take_should_stop(false, done, matches),
                    "offscreen frame stopped a take (done={done}, matches={matches})"
                );
            }
        }
    }

    // ---- #621: the camera seam -------------------------------------------
    //
    // `apply_camera_input` is now the *only* implementation of orbit and zoom, and
    // `on_window_event`'s `CursorMoved` / `MouseWheel` arms delegate into it. These pin the
    // maths, so the hoist cannot have changed how the visual feels — and so the editor, which
    // reaches the same function through `scene_input::SceneGesture`, provably moves the same
    // camera rather than a parallel one.

    // -----------------------------------------------------------------------
    // The Performer's catalog gate (organon#49 T5b)
    // -----------------------------------------------------------------------

    /// 🚨 **The guard that exists to stop a silent failure, pinned so it cannot silently
    /// regress.** `ensure_agent_worker` refuses an empty catalog rather than prompting the
    /// model with no vocabulary to actuate — the failure `organon-visual`'s manifest calls
    /// "a failure with no error attached to it".
    ///
    /// Both directions are asserted on purpose. The empty case alone would catch the check
    /// being **dropped** *or* **inverted** (an inverted check spawns on empty and fails this
    /// assertion) — but it would say nothing about the guard having been widened until it
    /// refuses everything, which is the same outage arriving from the other side. Two
    /// assertions, two ways to be wrong.
    ///
    /// ⚠️ No network here. The spawned worker blocks on its channel immediately; it reaches
    /// the localhost client only once a message is sent, and the `World` drop closes the
    /// sender, which ends the thread.
    #[test]
    fn an_empty_catalog_refuses_the_agent_worker_and_a_real_one_does_not() {
        let mut empty = World::new(Vec::new());
        empty.performer.ensure_agent_worker();
        assert!(
            empty.performer.tx.is_none(),
            "an empty catalog must not spawn a worker — a Performer with no vocabulary is \
             the silent failure this guard exists to prevent"
        );

        let mut stocked = World::new(vec![agent::CatSlot::num("glow")]);
        stocked.performer.ensure_agent_worker();
        assert!(
            stocked.performer.tx.is_some(),
            "a non-empty catalog must still spawn — the guard is a refusal for catalog-less \
             hosts, not a disabling of the Performer"
        );
    }

    #[test]
    fn a_drag_orbits_yaw_and_pitch() {
        let mut w = World::new(Vec::new());
        let (yaw, pitch) = (w.yaw, w.pitch);
        w.apply_camera_input(CameraInput::Orbit { dx: 10.0, dy: -4.0 });
        assert!((w.yaw - (yaw - 0.1)).abs() < 1e-6, "yaw {} from {yaw}", w.yaw);
        assert!((w.pitch - (pitch - 0.04)).abs() < 1e-6, "pitch {} from {pitch}", w.pitch);
    }

    /// The pitch clamp is what stops the camera going over the pole and inverting the view.
    #[test]
    fn pitch_is_clamped_at_both_ends() {
        let mut w = World::new(Vec::new());
        for _ in 0..500 {
            w.apply_camera_input(CameraInput::Orbit { dx: 0.0, dy: 100.0 });
        }
        assert!(w.pitch <= 1.5 + 1e-6, "pitch ran past the top: {}", w.pitch);
        for _ in 0..1000 {
            w.apply_camera_input(CameraInput::Orbit { dx: 0.0, dy: -100.0 });
        }
        assert!(w.pitch >= -1.5 - 1e-6, "pitch ran past the bottom: {}", w.pitch);
    }

    /// Scrolling up moves in, and the floor is near zero rather than at it — you can zoom all
    /// the way *through* the centre, which is deliberate and easy to "fix" back to a safe 1.0.
    #[test]
    fn the_wheel_zooms_in_and_holds_both_rails() {
        let mut w = World::new(Vec::new());
        let start = w.distance;
        w.apply_camera_input(CameraInput::Zoom { dy: 20.0 }); // one notch, the visual's unit
        assert!(w.distance < start, "scrolling up must move in");
        for _ in 0..5000 {
            w.apply_camera_input(CameraInput::Zoom { dy: 20.0 });
        }
        assert!(w.distance >= 0.1, "floor broke: {}", w.distance);
        for _ in 0..5000 {
            w.apply_camera_input(CameraInput::Zoom { dy: -20.0 });
        }
        assert!(w.distance <= 4000.0, "ceiling broke: {}", w.distance);
    }

    /// While riding the #187 rails the same gesture **leans inside the bore** instead of
    /// orbiting. That branch lived inside the winit event arm; hoisting it is exactly the kind
    /// of move that silently drops one side of an `if`.
    #[test]
    fn on_the_rails_a_drag_leans_instead_of_orbiting() {
        let mut w = World::new(Vec::new());
        w.rails_ride = true;
        let (yaw, pitch) = (w.yaw, w.pitch);
        w.apply_camera_input(CameraInput::Orbit { dx: 10.0, dy: 10.0 });
        assert_eq!(w.yaw, yaw, "the orbit must not move while riding");
        assert_eq!(w.pitch, pitch);
        assert!(w.rail_off.0 > 0.0, "dragging right must lean right");
        assert!(w.rail_off.1 < 0.0, "dragging down must lean the other way in Y");
    }

    #[test]
    fn a_presented_frame_still_stops_for_the_real_reasons() {
        // The gate must not have disabled the feature it guards.
        assert!(take_should_stop(true, true, true), "N-bar auto-stop must still fire");
        assert!(take_should_stop(true, false, false), "an invalidating change must still stop");
        assert!(!take_should_stop(true, false, true), "a good frame must not stop a take");
    }

    #[test]
    fn a_presented_target_carries_the_hosts_size_and_format() {
        let out = FrameOutput::of((1920, 1080), SDR, true);
        assert_eq!(out.size, (1920, 1080));
        assert_eq!(out.format, SDR);
        assert!(out.presented, "a swapchain image is presented");
    }

    #[test]
    fn offscreen_target_draws_at_the_callers_size_and_format() {
        // Deliberately different from the window's, and it must win: the caller
        // owns the texture, so its dimensions/format are the frame's truth.
        let out = FrameOutput::of((640, 360), HDR, false);
        assert_eq!(out.size, (640, 360));
        assert_eq!(out.format, HDR);
        assert!(!out.presented, "nothing to present into a caller's texture");
    }

    #[test]
    fn presented_is_the_callers_word_not_the_textures() {
        // The point of the split: a frame can resolve with `Gfx::win == None`.
        // `presented` is the CALLER'S word, not a property of the texture: same size, same
        // format, opposite display behaviour. That bit is the whole seam.
        let out = FrameOutput::of((256, 256), SDR, false);
        assert_eq!(out.size, (256, 256));
        assert!(!out.presented);
        let win = FrameOutput::of((256, 256), SDR, true);
        assert_eq!(frame_hdr_max(&win, true, 4.0, None), 4.0);
        assert_eq!(frame_hdr_max(&out, true, 4.0, None), 1.0);
    }

    // ── the UI layer's geometry (#593 T3) ──────────────────────────────────
    // `ui_scale_factor` replaced the `&winit::Window` the layer used to ask for its size and
    // scale, which makes it the ONLY thing telling the world what to lay the interface out at.
    // These four pin what that field means, because nothing downstream can recover from getting
    // it wrong — a silently-halved interface reads as a theme bug, not a missing input.

    /// **Absent is not 1.0.** A frame that states no scale factor draws no interface at all,
    /// rather than guessing a scale and laying the whole UI out at half size on a Retina
    /// display. This is `ui_window: None`'s meaning, carried across unchanged.
    #[test]
    fn no_scale_factor_draws_no_interface_rather_than_guessing_one() {
        let win = FrameOutput::of((2200, 1520), SDR, true);
        assert_eq!(ui_geometry(&win, None), None);
    }

    /// An offscreen frame is a picture of the *scene*. The frame mirror and the production
    /// recorder both go through this path, and an interface painted into a take would be a
    /// defect — so `presented` gates before the scale factor is even considered.
    #[test]
    fn an_offscreen_frame_never_lays_out_an_interface() {
        let off = FrameOutput::of((640, 360), SDR, false);
        assert_eq!(ui_geometry(&off, Some(2.0)), None);
    }

    /// A Retina scale reaches the layer intact. The failure this guards is the quiet one: a
    /// `2.0` degraded to `1.0` anywhere along the seam lays the interface out in physical
    /// pixels, i.e. at half size, with nothing erroring.
    #[test]
    fn a_retina_scale_reaches_the_layer_intact() {
        let win = FrameOutput::of((2200, 1520), SDR, true);
        let g = ui_geometry(&win, Some(2.0)).expect("presented + a stated scale draws");
        assert_eq!(g.physical_size, (2200, 1520));
        assert_eq!(g.scale(), 2.0);
        assert_eq!(g.logical_size(), (1100.0, 760.0));
    }

    /// The other half of the distinction: a scale that cannot be divided by falls back to 1.0
    /// rather than producing an infinite layout — but the frame still *draws*. Nonsense degrades;
    /// only *absence* means "no interface".
    #[test]
    fn a_nonsense_scale_degrades_to_one_but_still_draws() {
        let win = FrameOutput::of((1100, 760), SDR, true);
        for bad in [0.0, -1.0, f32::NAN] {
            let g = ui_geometry(&win, Some(bad)).expect("a stated scale still means draw");
            assert_eq!(g.scale(), 1.0, "scale {bad} must not reach a layout");
        }
    }

    #[test]
    fn hdr_headroom_reaches_the_window_but_never_an_offscreen_texture() {
        let win = FrameOutput::of((100, 100), HDR, true);
        let off = FrameOutput::of((100, 100), HDR, false);
        // Window + HDR on → the display's measured EDR headroom (unchanged).
        assert_eq!(frame_hdr_max(&win, true, 2.5, None), 2.5);
        // Window + HDR off → SDR tone-map (unchanged).
        assert_eq!(frame_hdr_max(&win, false, 2.5, None), 1.0);
        // Recording overrides with the mastering headroom (#430, unchanged).
        assert_eq!(frame_hdr_max(&win, true, 2.5, Some(4.0)), 4.0);
        // Offscreen: SDR no matter what the window/display would have done —
        // there is no CAMetalLayer, so extended-range values would be junk to
        // whoever consumes the texture.
        assert_eq!(frame_hdr_max(&off, true, 2.5, None), 1.0);
        assert_eq!(frame_hdr_max(&off, true, 2.5, Some(4.0)), 1.0);
    }

    #[test]
    fn wide_gamut_expansion_only_runs_against_a_tagged_surface() {
        let win = FrameOutput::of((100, 100), HDR, true);
        let off = FrameOutput::of((100, 100), HDR, false);
        assert_eq!(frame_gamut(&win, true, true), 1.0);
        assert_eq!(frame_gamut(&win, true, false), 0.0);
        assert_eq!(frame_gamut(&win, false, true), 0.0);
        // Only a presented surface is ever tagged Rec.2020 (#119).
        assert_eq!(frame_gamut(&off, true, true), 0.0);
    }

    /// Append-and-drain (finding #3): a burst of Sends appended before the visual reads
    /// the counter must ALL drain, in order, once each — none dropped, none replayed.
    #[test]
    fn agent_chat_drain_takes_every_new_line_once() {
        // Two messages appended, none consumed yet → both drain.
        let body = "warm intro\nfaster now\n";
        let (msgs, cursor) = agent_chat_drain(body, 0);
        assert_eq!(msgs, vec!["warm intro".to_string(), "faster now".to_string()]);
        assert_eq!(cursor, 2);
        // A third appended after the cursor → only the new one drains.
        let body = "warm intro\nfaster now\nnow cool it\n";
        let (msgs, cursor) = agent_chat_drain(body, cursor);
        assert_eq!(msgs, vec!["now cool it".to_string()]);
        assert_eq!(cursor, 3);
        // Nothing new → empty drain, cursor unchanged.
        let (msgs, cursor) = agent_chat_drain(body, cursor);
        assert!(msgs.is_empty());
        assert_eq!(cursor, 3);
    }

    /// Edge-detect seeding (finding #6): seeding the cursor to the current line count means
    /// a visual restart against a pre-populated sidecar replays nothing, yet a genuinely
    /// new line after the restart still drains.
    #[test]
    fn agent_chat_drain_seeded_cursor_skips_history() {
        let existing = "old one\nold two\n";
        // Simulate startup seeding: cursor := current non-empty line count.
        let seed = existing.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(seed, 2);
        let (msgs, _) = agent_chat_drain(existing, seed);
        assert!(msgs.is_empty(), "restart must not replay history");
        // A new send appends a line; only it drains.
        let after = "old one\nold two\nbrand new\n";
        let (msgs, cursor) = agent_chat_drain(after, seed);
        assert_eq!(msgs, vec!["brand new".to_string()]);
        assert_eq!(cursor, 3);
    }

    /// The tempo-synced Maxwell oscillation is an LFO on the beat clock: one full
    /// there-and-back (τ radians) per note division, wrapped to `[0, τ)`.
    #[test]
    fn maxwell_osc_phase_locks_to_division() {
        use std::f32::consts::PI;
        let bpb = 4.0;
        // Quarter = one full cycle per beat: 0 at the downbeat, π at mid-beat.
        assert!(maxwell_osc_phase(0.0, OscDivision::Quarter, bpb).abs() < 1e-4);
        assert!((maxwell_osc_phase(0.5, OscDivision::Quarter, bpb) - PI).abs() < 1e-3);
        // Eighth is twice as fast: half a beat = one full cycle → wraps back to ~0.
        assert!(maxwell_osc_phase(0.5, OscDivision::Eighth, bpb).abs() < 1e-3);
        // Bar completes exactly one cycle over `beats_per_bar` beats → wraps to ~0.
        assert!(maxwell_osc_phase(bpb as f64, OscDivision::Bar, bpb).abs() < 1e-3);
        // 2-Bar is half Bar's rate: at one bar it is only mid-cycle (π).
        assert!((maxwell_osc_phase(bpb as f64, OscDivision::TwoBar, bpb) - PI).abs() < 1e-3);
    }

    /// #206 Tier 1: the scenery membrane loft skins a Grid archetype, appends
    /// its vertex range past prior geometry (the two-worlds transition), and
    /// declines a Streamlines archetype (which falls back to swept tubes).
    #[test]
    fn scenery_loft_skins_grid_and_declines_streamlines() {
        let mut spec = math::RailsSpec::from_slots(&ipc::Shared::default().rails);
        let mut strands = math::Strands::new();
        // Throat (archetype 0) is Grid.
        spec.archetype = 0;
        let topo = math::rails_strands(&spec, 12.0, math::RAILS_FULL_SPAN, 0.0, 0.0, 0, 0.0, &mut strands);
        assert_eq!(topo, math::Topology::Grid);
        let (mut p, mut n, mut c, mut i) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let ok = loft_scenery_append(&strands, math::rails_loft_dims(&spec), topo, 0, 0.0, &mut p, &mut n, &mut c, &mut i);
        assert!(ok, "Grid archetype must loft");
        assert!(!p.is_empty() && !i.is_empty(), "loft produced a mesh");
        assert_eq!(p.len(), n.len());
        assert_eq!(p.len(), c.len());
        let max_idx = *i.iter().max().unwrap();
        assert!((max_idx as usize) < p.len(), "indices in range");
        let (v0, i0) = (p.len(), i.len());

        // Append a second world: its indices must be offset past the first's
        // vertex range (the quantized-transition two-worlds skin).
        let ok2 = loft_scenery_append(&strands, math::rails_loft_dims(&spec), topo, 0, 0.0, &mut p, &mut n, &mut c, &mut i);
        assert!(ok2);
        assert_eq!(p.len(), 2 * v0, "second world appended its verts");
        assert_eq!(i.len(), 2 * i0);
        let min_second = i[i0..].iter().copied().min().unwrap();
        assert!(min_second >= v0 as u32, "second world's indices are offset past the first");

        // Gates (archetype 2) is Streamlines → no loft, nothing appended.
        spec.archetype = 2;
        let topo_s = math::rails_strands(&spec, 12.0, math::RAILS_FULL_SPAN, 0.0, 0.0, 0, 0.0, &mut strands);
        assert_eq!(topo_s, math::Topology::Streamlines);
        let (before_p, before_i) = (p.len(), i.len());
        let ok3 = loft_scenery_append(&strands, math::rails_loft_dims(&spec), topo_s, 0, 0.0, &mut p, &mut n, &mut c, &mut i);
        assert!(!ok3, "Streamlines archetype must decline to loft");
        assert_eq!(p.len(), before_p, "nothing appended on decline");
        assert_eq!(i.len(), before_i);
    }

    /// #206 Tier 2: the combined 40-slot latch block dispatches Terra (rails
    /// timing [0..24] ++ terra landform [24..40]) into a Grid that skins, and
    /// Zone still dispatches its corridor from the same block.
    #[test]
    fn gen_scenery_world_dispatches_terra_and_zone() {
        let d = ipc::Shared::default();
        let mut blk = [0.0f32; 40];
        blk[..24].copy_from_slice(&d.rails);
        blk[24..].copy_from_slice(&d.terra);

        // Terra: Grid, one strand per lateral sample, skinnable.
        let mut out = math::Strands::new();
        let (topo, dims) =
            gen_scenery_world(&blk, true, 12.0, math::RAILS_FULL_SPAN, 0.0, 0.0, 0, 0.0, &mut out);
        assert_eq!(topo, math::Topology::Grid);
        let lat = math::RailsSpec::from_slots(&blk[..24]).ring_n.max(3);
        assert_eq!(dims, (lat, 1));
        assert_eq!(out.len(), lat);
        let (mut p, mut n, mut c, mut i) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        assert!(loft_scenery_append(&out, dims, topo, 0, 0.0, &mut p, &mut n, &mut c, &mut i));
        assert!(!i.is_empty(), "Terra membrane skinned");

        // Zone from the same block: still the corridor (throat = Grid).
        let mut zout = math::Strands::new();
        let (ztopo, _) =
            gen_scenery_world(&blk, false, 12.0, math::RAILS_FULL_SPAN, 0.0, 0.0, 0, 0.0, &mut zout);
        assert_eq!(ztopo, math::Topology::Grid);
        assert!(!zout.is_empty());
    }

    #[test]
    fn jelly_stroke_is_a_sustained_pump() {
        // Rest at the beat boundaries, full squeeze at the contraction peak (~0.35),
        // and a broad sustained high region (unlike a sharp spike).
        assert!(jelly_stroke(0.0) < 0.05, "rest at the downbeat");
        assert!(jelly_stroke(1.0 - 1e-4) < 0.1, "near rest by the next beat");
        assert!(jelly_stroke(0.35) > 0.95, "full squeeze at the peak");
        // Sustained: still meaningfully contracted a third of a beat after the peak.
        assert!(jelly_stroke(0.6) > 0.4, "recovery is gradual, not instant");
        // Monotone rise into the peak.
        assert!(jelly_stroke(0.2) < jelly_stroke(0.3));
    }

    #[test]
    fn rails_latch_quantizes_geometry_changes() {
        let mut base = [0.0f32; 24];
        base[0] = 8.0; // speed (live)
        base[1] = 6.0; // bore (geometry)
        base[3] = 3.0; // change every = 32 beats
        base[14] = 0.3; // swell (geometry)
        let (mut active, mut pending) = ([0.0f32; 24], None);
        // Fresh (re)entry adopts the live block wholesale.
        rails_latch_step(&mut active, &mut pending, &base, 10.0, true);
        assert_eq!(active, base);
        assert!(pending.is_none());
        // A LIVE slot (speed — the pulse pump) passes through instantly.
        let mut live = base;
        live[0] = 12.0;
        rails_latch_step(&mut active, &mut pending, &live, 10.5, false);
        assert_eq!(active[0], 12.0);
        assert!(pending.is_none(), "live slots must not latch");
        // A geometry change latches to the NEXT 32-beat boundary.
        live[14] = 0.9;
        rails_latch_step(&mut active, &mut pending, &live, 10.6, false);
        assert_eq!(active[14], 0.3, "geometry must wait for the bar");
        assert_eq!(pending.unwrap().1, 32.0);
        // Further tweaks keep the SAME boundary (latest values win on the bar).
        live[1] = 9.0;
        rails_latch_step(&mut active, &mut pending, &live, 11.0, false);
        assert_eq!(pending.unwrap().1, 32.0);
        assert_eq!(pending.unwrap().0[1], 9.0);
        // Reverting before the bar cancels the transition.
        live = base;
        live[0] = 12.0; // live slot may differ — still counts as a revert
        rails_latch_step(&mut active, &mut pending, &live, 12.0, false);
        assert!(pending.is_none(), "a pre-bar revert must cancel");
        // Change again late in the phrase, then cross the boundary: applied.
        live[14] = 0.9;
        rails_latch_step(&mut active, &mut pending, &live, 31.9, false);
        assert_eq!(pending.unwrap().1, 32.0);
        rails_latch_step(&mut active, &mut pending, &live, 32.0, false);
        assert_eq!(active[14], 0.9, "crossing the bar applies the pending world");
        assert!(pending.is_none());
        // A change landing EXACTLY on a boundary applies that same downbeat
        // (ceil), not a full phrase later.
        live[1] = 4.0;
        rails_latch_step(&mut active, &mut pending, &live, 64.0, false);
        assert_eq!(active[1], 4.0, "an on-the-bar change lands on that bar");
        assert!(pending.is_none());
    }

    #[test]
    fn drs_lowers_when_slow_raises_when_fast_holds_in_band() {
        let target = 1000.0 / 60.0; // 16.67 ms
        // Slow frame (33 ms ≈ 30 FPS) → scale drops.
        assert!(drs_adjust(1.0, 33.0, target) < 1.0);
        // Fast frame (8 ms ≈ 120 FPS) at reduced scale → scale climbs back.
        assert!(drs_adjust(0.5, 8.0, target) > 0.5);
        // On target → unchanged.
        assert_eq!(drs_adjust(0.7, target, target), 0.7);
        // Clamped to the floor even when hopelessly slow.
        assert!(drs_adjust(0.25, 100.0, target) >= 0.25);
        // Never exceeds native.
        assert!(drs_adjust(1.0, 1.0, target) <= 1.0);
    }

    fn shared(sync: bool, playing: bool, host_pos: f32, host_tempo: f32, manual: f32) -> ipc::Shared {
        let mut s = ipc::Shared::default();
        s.tempo_sync = sync as u32;
        s.tempo = manual;
        s.transport = [
            if playing { 1.0 } else { 0.0 },
            host_pos,
            host_tempo,
            if host_tempo > 0.0 { 1.0 } else { 0.0 },
        ];
        s
    }

    #[test]
    fn free_runs_off_manual_tempo_with_no_host() {
        // No host tempo → uses the manual slider, regardless of sync flag.
        let s = shared(true, false, 0.0, 0.0, 120.0);
        let (mut beat, mut prev) = (0.0, 0.0);
        // 1 second at 120bpm = 2 beats.
        for _ in 0..60 {
            advance_beat_clock(&mut beat, &mut prev, &s, 1.0 / 60.0, false);
        }
        assert!((beat - 2.0).abs() < 1e-3, "beat={beat}");
    }

    #[test]
    fn host_tempo_followed_even_when_sync_off() {
        // Host SOURCE follows the host tempo whenever the host provides one; the
        // sync (PLL-lock) toggle only tightens the phase, not the rate. So even with
        // sync OFF, the beat runs at the host 60bpm (1 beat/s), not the manual 120.
        let s = shared(false, true, 0.0, 60.0, 120.0);
        let (mut beat, mut prev) = (0.0, 0.0);
        for _ in 0..60 {
            advance_beat_clock(&mut beat, &mut prev, &s, 1.0 / 60.0, false);
        }
        assert!((beat - 1.0).abs() < 1e-3, "beat={beat}");
    }

    #[test]
    fn pll_pulls_phase_toward_host() {
        // Host plays at 120bpm starting half a beat ahead; the visual starts at
        // 0. As the host advances each frame, the PLL should drive the phase
        // error to ~0 within a couple of seconds (no steady-state offset because
        // both clocks run at the same rate).
        let dt = 1.0 / 60.0;
        let mut host = 0.5f64; // host's beat position, advancing
        let mut beat = 0.0f64;
        let mut prev = 0.0f64;
        for _ in 0..240 {
            host += dt * 120.0 / 60.0;
            let mut s = shared(true, true, 0.0, 120.0, 120.0);
            s.transport[1] = (host.rem_euclid(1024.0)) as f32;
            advance_beat_clock(&mut beat, &mut prev, &s, dt, false);
        }
        // Shortest distance between the two phases should have collapsed.
        let mut err = (host - beat).rem_euclid(1.0);
        if err > 0.5 {
            err -= 1.0;
        }
        assert!(err.abs() < 0.02, "phase did not lock: err={err}");
    }

    #[test]
    fn fixed_timestep_bypasses_pll() {
        // #430 perfect capture: even with host present + sync on + advancing, `fixed = true`
        // must free-run (no phase pull) — the fixed-timestep clock isn't real time, so
        // locking to the still-real-time host would fight it.
        let dt = 1.0 / 60.0;
        let mut host = 0.5f64;
        let mut beat = 0.0f64;
        let mut prev = 0.0f64;
        for _ in 0..240 {
            host += dt * 120.0 / 60.0;
            let mut s = shared(true, true, 0.0, 120.0, 120.0);
            s.transport[1] = (host.rem_euclid(1024.0)) as f32;
            advance_beat_clock(&mut beat, &mut prev, &s, dt, true); // fixed
        }
        // Pure free-run: 240 · (1/60) · 2 = 8.0 beats, ignoring the host's 0.5-beat head start
        // (a PLL would have pulled it toward ~8.5).
        assert!((beat - 8.0).abs() < 1e-3, "fixed clock should free-run, beat={beat}");
    }

    #[test]
    fn record_bars_cycle_wraps() {
        // #430: 8 → 16 → 32 → 64 → Free(0) → 8, and any stray value returns to 8.
        // #430 chunk mode cycles a PHRASE in beats — a different unit from the take length
        // in bars, and the motivating default (8 beats = two bars of 4/4) is in the cycle.
        assert_eq!(next_phrase_beats(4.0), 8.0);
        assert_eq!(next_phrase_beats(8.0), 16.0);
        assert_eq!(next_phrase_beats(16.0), 32.0);
        assert_eq!(next_phrase_beats(32.0), 4.0);
        assert_eq!(next_phrase_beats(7.0), 8.0); // anything unexpected → the default phrase
        // Every phrase divides 1024, so the grid survives the host's `pos_beats` wrap.
        let mut p = 4.0;
        for _ in 0..4 {
            assert_eq!(1024.0_f64 % p, 0.0, "phrase {p} must divide the mod-1024 wrap");
            p = next_phrase_beats(p);
        }

        assert_eq!(next_record_bars(8), 16);
        assert_eq!(next_record_bars(16), 32);
        assert_eq!(next_record_bars(32), 64);
        assert_eq!(next_record_bars(64), 0);
        assert_eq!(next_record_bars(0), 8);
        assert_eq!(next_record_bars(5), 8);
        assert_eq!(record_len_label(0), "Free (press R to stop)");
        assert_eq!(record_len_label(16), "16 bars");
    }

    #[test]
    fn no_phase_correction_when_stopped() {
        // Host present but not playing → pure free-run, no jump toward host_pos.
        let s = shared(true, false, 0.5, 120.0, 120.0);
        let (mut beat, mut prev) = (0.0, 0.0);
        advance_beat_clock(&mut beat, &mut prev, &s, 1.0 / 60.0, false);
        let expected = (120.0 / 60.0) * (1.0 / 60.0);
        assert!((beat - expected).abs() < 1e-9, "beat={beat}");
    }

    #[test]
    fn camera_drifts_at_base_speed_with_no_kick() {
        let (mut phase, mut vel) = (0.0, 0.0);
        // base 0.1 cyc/beat, no kick; advance exactly one beat.
        advance_camera(&mut phase, &mut vel, 0.1, 0.0, 0.4, 1.0, 0.0);
        assert!((phase - 0.1).abs() < 1e-9, "phase={phase}");
        assert_eq!(vel, 0.0);
    }

    #[test]
    fn beat_kick_adds_momentum_that_decays() {
        let (mut phase, mut vel) = (0.0, 0.0);
        // One beat crossing kicks velocity; with damping<1 and no further
        // kicks it must bleed back toward zero.
        advance_camera(&mut phase, &mut vel, 0.0, 0.5, 0.4, 1.0, 1.0);
        let vel_after_kick = vel;
        assert!(vel_after_kick > 0.0, "kick should add velocity: {vel_after_kick}");
        for _ in 0..50 {
            advance_camera(&mut phase, &mut vel, 0.0, 0.0, 0.4, 1.0, 0.0);
        }
        assert!(vel < vel_after_kick * 0.05, "velocity did not decay: {vel}");
    }

    #[test]
    fn path_offsets_are_sane() {
        use std::f32::consts::FRAC_PI_2;
        // Off → identity (no motion, unit dist/fov).
        let o = camera_path_offset(0, 0.3, 1.0);
        assert_eq!((o.dyaw, o.dpitch, o.dist), (0.0, 0.0, 1.0));
        assert_eq!(o.fov_mul, 1.0);
        // Horizontal circle: quarter phase ≈ 90° yaw, level, no zoom.
        let o = camera_path_offset(1, 0.25, 1.0);
        assert!((o.dyaw - FRAC_PI_2).abs() < 1e-4, "yaw={}", o.dyaw);
        assert!(o.dpitch.abs() < 1e-6 && (o.dist - 1.0).abs() < 1e-6);
        // Figure-eight passes through the origin at phase 0.
        let o = camera_path_offset(3, 0.0, 1.0);
        assert!(o.dyaw.abs() < 1e-6 && o.dpitch.abs() < 1e-6);
        // amount = 0 kills the swing on the oscillating paths.
        let o = camera_path_offset(2, 0.25, 0.0);
        assert!(o.dpitch.abs() < 1e-6, "amount=0 should flatten: {}", o.dpitch);
    }

    // ---- #307 Tier 1: sequencer + dolly + tempo source ----

    #[test]
    fn sequencer_series_cycles_on_bar_boundaries() {
        let mut seq = SeqState::new();
        // 2 bars per shot, Series, no holding. First observation latches, no change.
        seq.step(0.0, 2.0, 0, 0.0);
        let first = seq.cur;
        // Still inside the first 2-bar block → no change.
        seq.step(1.5, 2.0, 0, 0.0);
        assert_eq!(seq.cur, first, "changed mid-block");
        // Cross into the next block → advance to the next path in series.
        seq.step(2.0, 2.0, 0, 0.0);
        assert_ne!(seq.cur, first, "did not advance on the boundary");
        assert_eq!(seq.prev, first, "prev should be the outgoing move");
        // Series order is deterministic: SEQ_PATHS cycles 1→2→3→4→1.
        assert_eq!(seq.cur, next_shot(first, 0, &mut 0));
    }

    #[test]
    fn sequencer_random_never_repeats_immediately() {
        let mut rng = 0xC0FF_EE00u32;
        let mut cur = 1u32;
        for _ in 0..500 {
            let n = next_shot(cur, 1, &mut rng);
            assert_ne!(n, cur, "random picked an immediate repeat");
            assert!(SEQ_PATHS.contains(&n));
            cur = n;
        }
    }

    #[test]
    fn glide_progress_ramps_then_saturates() {
        let mut seq = SeqState::new();
        seq.step(0.0, 4.0, 0, 0.0); // latch block 0
        seq.step(4.0, 4.0, 0, 0.0); // boundary at bar 4
        // Half a bar into a 1-bar glide → ~0.5; a Cut (0 bars) → 1 immediately.
        assert!((seq.glide_t(4.5, 1.0) - 0.5).abs() < 1e-5);
        assert_eq!(seq.glide_t(4.0, 0.0), 1.0, "cut should be instant");
        assert_eq!(seq.glide_t(6.0, 1.0), 1.0, "glide should saturate at 1");
    }

    #[test]
    fn dolly_is_inert_at_zero_depth_and_breathes_otherwise() {
        // Depth 0 → always 1.0 (today's framing).
        for p in [0.0, 0.3, 0.7, 1.0] {
            assert_eq!(dolly_factor(4.0, 0.0, 0, p), 1.0);
        }
        // Sine, depth 0.5: quarter period = far point (1 + 0.5).
        assert!((dolly_factor(4.0, 0.5, 0, 1.0) - 1.5).abs() < 1e-5);
        // Three-quarter period = near point (1 − 0.5).
        assert!((dolly_factor(4.0, 0.5, 0, 3.0) - 0.5).abs() < 1e-5);
        // Mean over a whole period ≈ 1 (mean-centred breath).
        let mean: f64 = (0..1000)
            .map(|i| dolly_factor(4.0, 0.5, 0, i as f64 * 4.0 / 1000.0) as f64)
            .sum::<f64>()
            / 1000.0;
        assert!((mean - 1.0).abs() < 1e-2, "dolly not mean-centred: {mean}");
    }

    #[test]
    fn tempo_source_selects_bpm() {
        // Host (default): tempo_sync locks to the host tempo when present.
        let mut s = ipc::Shared::default();
        s.tempo_sync = 1;
        s.transport = [1.0, 0.0, 140.0, 1.0];
        s.cam_clock[0] = 0.0; // Host
        assert!((active_bpm(&s) - 140.0).abs() < 1e-3);
        // Manual: always the dial, ignoring the host.
        s.cam_clock[0] = 2.0;
        s.tempo = 100.0;
        assert!((active_bpm(&s) - 100.0).abs() < 1e-3);
        // Audio: the detected BPM; falls back to the dial before the first lock.
        s.cam_clock[0] = 1.0;
        s.cam_audio[0] = 0.0;
        assert!((active_bpm(&s) - 100.0).abs() < 1e-3, "should fall back to dial");
        s.cam_audio[0] = 128.0;
        assert!((active_bpm(&s) - 128.0).abs() < 1e-3, "should use detected BPM");
    }

    // ---- #307 Tier 2: moves + framing + order ----

    #[test]
    fn tier2_moves_produce_their_signature_axes() {
        // Truck slides laterally with no orbit; Push/Pull only changes distance;
        // Pendulum leans (rolls); Drift stays bounded.
        let truck = camera_path_offset(7, 0.25, 1.0);
        assert!(truck.lat_x.abs() > 0.1 && truck.dyaw.abs() < 1e-6, "truck should truck");
        let pushpull = camera_path_offset(8, 0.5, 1.0);
        assert!(pushpull.dist < 0.95, "push/pull should dolly in: {}", pushpull.dist);
        let pend = camera_path_offset(6, 0.0, 1.0);
        assert!(pend.roll.abs() > 1e-4, "pendulum should lean");
        for i in 0..200 {
            let d = camera_path_offset(10, i as f64 * 0.37, 1.0);
            assert!(d.dyaw.abs() < 1.0 && d.dpitch.abs() < 1.0, "drift unbounded");
        }
    }

    #[test]
    fn seq_blend_interpolates_orbit_cam_and_sequencer() {
        // The base orbit-cam (HCircle, id 1) at phase 0.25 = 90° yaw; a sequencer
        // move (VCircle, id 2) has 0 yaw. Blending should interpolate the yaw:
        // mix 0 → base only, mix 1 → sequencer only, 0.5 → halfway.
        let base = camera_path_offset(1, 0.25, 1.0);
        let seq = camera_path_offset(2, 0.25, 1.0);
        let at = |mix: f32| CamOffset::mix(base, seq, mix).dyaw;
        assert!((at(0.0) - base.dyaw).abs() < 1e-6, "mix 0 must be pure orbit-cam");
        assert!((at(1.0) - seq.dyaw).abs() < 1e-6, "mix 1 must be pure sequencer");
        let mid = at(0.5);
        assert!(
            (mid - 0.5 * (base.dyaw + seq.dyaw)).abs() < 1e-6,
            "mix 0.5 must be the midpoint: {mid}"
        );
    }

    #[test]
    fn drift_noise_is_two_sided_and_centred() {
        // #316 Bugbot: the hash must span [-1, 1), not [-1, 0) — otherwise Handheld
        // Drift only ever pushes one way. Sample the lattice and check both signs +
        // a mean near zero.
        let (mut lo, mut hi, mut sum) = (f64::MAX, f64::MIN, 0.0);
        let n = 4000;
        for i in 0..n {
            let v = drift_noise(i as f64 * 0.5 + 0.13, 1);
            lo = lo.min(v);
            hi = hi.max(v);
            sum += v;
        }
        assert!(lo < -0.3, "drift never goes clearly negative: min={lo}");
        assert!(hi > 0.3, "drift never goes clearly positive: max={hi}");
        assert!((sum / n as f64).abs() < 0.1, "drift is biased off-centre: mean={}", sum / n as f64);
    }

    #[test]
    fn shuffle_covers_all_moves_before_repeating() {
        let mut seq = SeqState::new();
        let mut seen = std::collections::HashSet::new();
        seq.step(0.0, 1.0, 2, 0.0); // latch
        // Over one full bag length, every move should appear exactly once.
        for b in 1..=SEQ_PATHS.len() {
            seq.step(b as f64, 1.0, 2, 0.0);
            seen.insert(seq.cur);
        }
        assert_eq!(seen.len(), SEQ_PATHS.len(), "shuffle did not cover all moves: {seen:?}");
    }

    #[test]
    fn hold_probability_one_pins_the_shot() {
        let mut seq = SeqState::new();
        seq.step(0.0, 1.0, 1, 1.0); // latch
        let pinned = seq.cur;
        for b in 1..50 {
            seq.step(b as f64, 1.0, 1, 1.0); // always hold
            assert_eq!(seq.cur, pinned, "hold_prob=1 should never change the shot");
            assert!(!seq.just_changed);
            // A held boundary must NOT restart the Glide crossfade: with the shot
            // pinned, glide_t stays saturated at 1 across every bar (no snap back to
            // prev). (Regression guard for the #316 Bugbot "Hold resets glide" bug.)
            assert_eq!(
                seq.glide_t(b as f64, 1.0),
                1.0,
                "held shot must keep glide_t saturated (no re-crossfade)"
            );
        }
    }

    #[test]
    fn weighted_order_never_repeats_immediately() {
        let mut rng = 0xBEEF_1234u32;
        let mut cur = 5u32;
        for _ in 0..500 {
            let n = next_shot(cur, 3, &mut rng);
            assert_ne!(n, cur, "weighted picked an immediate repeat");
            assert!(SEQ_PATHS.contains(&n));
            cur = n;
        }
    }

    // ---- #307 Tier 3: storyboard ----

    fn story_arr(count: u32, mode: u32, seed: i32, shots: &[(u32, f32, f32)]) -> [f32; 24] {
        let mut a = [0.0f32; 24];
        a[0] = 1.0; // enabled
        a[1] = count as f32;
        a[2] = mode as f32;
        a[3] = seed as f32;
        for (k, (p, b, r)) in shots.iter().enumerate() {
            a[8 + k * 4] = *p as f32;
            a[8 + k * 4 + 1] = *b;
            a[8 + k * 4 + 2] = *r;
        }
        a
    }

    #[test]
    fn storyboard_advances_on_each_shots_bars() {
        // Shot 0 holds 2 bars, shot 1 holds 4 bars (Series).
        let a = story_arr(4, 0, 1, &[(1, 2.0, 1.0), (4, 4.0, 1.0), (3, 2.0, 1.0), (5, 8.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a); // init → slot 0
        assert_eq!(st.cur, 0);
        st.step(1.0, &a);
        assert_eq!(st.cur, 0, "changed before its 2 bars elapsed");
        st.step(2.0, &a);
        assert_eq!(st.cur, 1, "did not advance after 2 bars");
        // Slot 1 holds 4 bars → next change at bar 6.
        st.step(5.0, &a);
        assert_eq!(st.cur, 1);
        st.step(6.0, &a);
        assert_eq!(st.cur, 2, "did not advance after slot 1's 4 bars");
        assert_eq!(st.prev, 1);
    }

    #[test]
    fn storyboard_manual_next_fires_on_the_next_bar() {
        // Long shots so only the manual trigger changes them.
        let a = story_arr(4, 0, 1, &[(1, 16.0, 1.0), (4, 16.0, 1.0), (3, 16.0, 1.0), (5, 16.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a);
        st.step(0.5, &a);
        assert_eq!(st.cur, 0);
        // Bump the trigger mid-bar → armed, not yet fired.
        let mut a2 = a;
        a2[4] = 1.0;
        st.step(0.6, &a2);
        assert_eq!(st.cur, 0, "manual next fired before the bar line");
        // Cross the bar line → fires.
        st.step(1.0, &a2);
        assert_eq!(st.cur, 1, "manual next did not fire on the next bar");
    }

    #[test]
    fn storyboard_seed_is_reproducible() {
        let a = story_arr(4, 1, 42, &[(1, 1.0, 1.0), (4, 1.0, 1.0), (3, 1.0, 1.0), (5, 1.0, 1.0)]);
        let run = || {
            let mut st = StoryState::new();
            let mut seq = Vec::new();
            let mut bar = 0.0f64;
            st.step(bar, &a);
            for _ in 0..20 {
                bar += 1.0;
                st.step(bar, &a);
                seq.push(st.cur);
            }
            seq
        };
        assert_eq!(run(), run(), "same seed must replay identically");
    }

    #[test]
    fn storyboard_single_shot_never_flags_a_change() {
        // #318 Bugbot: a 1-shot Series storyboard re-picks slot 0 every boundary;
        // it must NOT flag just_changed (that would hitch phrase-locked facing).
        let a = story_arr(1, 0, 1, &[(1, 1.0, 1.0), (0, 0.0, 0.0), (0, 0.0, 0.0), (0, 0.0, 0.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a);
        for b in 1..12 {
            st.step(b as f64, &a);
            assert_eq!(st.cur, 0, "single-shot storyboard must stay on slot 0");
            assert!(!st.just_changed, "single-shot must never flag a change (bar {b})");
        }
    }

    #[test]
    fn storyboard_clamps_prev_when_count_shrinks() {
        // #318 Bugbot: shrinking the count must clamp BOTH cur and prev into range,
        // so the crossfade never blends a removed slot.
        let a4 = story_arr(4, 0, 1, &[(1, 1.0, 1.0), (4, 1.0, 1.0), (3, 1.0, 1.0), (5, 1.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a4);
        // Run to slot 3 → prev 2, cur 3.
        for b in 1..=3 {
            st.step(b as f64, &a4);
        }
        assert!(st.cur >= 2 && st.prev >= 1, "setup: expected high slots, got {}/{}", st.prev, st.cur);
        // Shrink to 2 shots → both indices must fall in range.
        let a2 = story_arr(2, 0, 1, &[(1, 1.0, 1.0), (4, 1.0, 1.0), (3, 1.0, 1.0), (5, 1.0, 1.0)]);
        st.step(3.1, &a2);
        assert!(st.cur < 2, "cur not clamped: {}", st.cur);
        assert!(st.prev < 2, "prev not clamped: {}", st.prev);
    }

    #[test]
    fn storyboard_survives_a_backward_seek() {
        // #318 Bugbot: after the bar clock jumps backward (host seek), scheduled
        // advances must still fire — not stall waiting to re-reach the old boundary.
        let a = story_arr(4, 0, 1, &[(1, 2.0, 1.0), (4, 2.0, 1.0), (3, 2.0, 1.0), (5, 2.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a);
        st.step(10.0, &a); // playing deep into the timeline
        let after = st.cur;
        // Seek back to bar 1 → re-anchor; then two bars later must advance again.
        st.step(1.0, &a);
        st.step(1.5, &a);
        st.step(3.0, &a); // 2 bars past the re-anchored boundary
        assert_ne!(st.cur, after, "storyboard stalled after a backward seek");
    }

    #[test]
    fn storyboard_reseed_clears_pending_next() {
        // #318 Bugbot: a "next shot" armed before a re-seed must not fire immediately
        // after the playlist restarts (it should honour the fresh hold).
        let a = story_arr(4, 0, 1, &[(1, 16.0, 1.0), (4, 16.0, 1.0), (3, 16.0, 1.0), (5, 16.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a);
        // Arm the manual trigger.
        let mut a_trig = a;
        a_trig[4] = 1.0;
        st.step(0.5, &a_trig);
        // Re-seed (different seed) while the trigger is armed.
        let mut a_seed = a_trig;
        a_seed[3] = 99.0;
        st.step(0.6, &a_seed); // reinit, must clear pending_next
        // Cross a bar line → must NOT advance (pending was cleared, shots are 16 bars).
        st.step(1.0, &a_seed);
        assert_eq!(st.cur, 0, "armed next survived the re-seed and skipped the hold");
    }

    #[test]
    fn storyboard_armed_next_does_not_double_advance() {
        // #318 Bugbot: a manual "next" armed mid-bar must be CONSUMED when the shot's
        // scheduled duration elapses first — otherwise the still-armed trigger fires a
        // SECOND advance on the next bar line (two shot skips from one button press).
        // Shot 0 is 1.5 bars so the scheduled advance lands mid-bar (bi unchanged),
        // exercising the scheduled-then-armed order the earlier fixes didn't cover.
        let a = story_arr(4, 0, 1, &[(1, 1.5, 1.0), (4, 8.0, 1.0), (3, 8.0, 1.0), (5, 8.0, 1.0)]);
        let mut st = StoryState::new();
        st.step(0.0, &a); // init → slot 0, last_bar_int = 0
        st.step(1.0, &a); // cross bar 1; shot is 1.5 bars so no advance yet
        // Arm the manual trigger mid-bar (bi stays 1 → not fired immediately).
        let mut a_trig = a;
        a_trig[4] = 1.0;
        st.step(1.2, &a_trig);
        assert_eq!(st.cur, 0, "armed trigger must not fire mid-bar");
        // Scheduled duration (1.5 bars) elapses within the same bar → one advance,
        // which must also consume the armed trigger.
        st.step(1.5, &a_trig);
        assert_eq!(st.cur, 1, "scheduled advance should move to slot 1");
        // Next bar line: the (now-consumed) trigger must NOT advance again.
        st.step(2.0, &a_trig);
        assert_eq!(st.cur, 1, "armed next double-advanced past the scheduled change");
    }

    fn empty_pv() -> ParamValues {
        ParamValues {
            loop_count: Vec3::ZERO,
            loop_count_q: 0.0,
            rot_amp: Vec3::ZERO,
            trans_amp: Vec3::ZERO,
            trans_mod: Vec3::ZERO,
            scale_amp: 0.2,
        }
    }

    #[test]
    fn routing_applies_scaled_delta_to_target() {
        let mut s = ipc::Shared::default();
        let mut pv = empty_pv();
        // Target 1 = Scale Amp, span 0.3; signal 0.5 → +0.15.
        apply_mod(&mut s, &mut pv, 1, 0.5);
        assert!((pv.scale_amp - 0.35).abs() < 1e-6, "scale_amp={}", pv.scale_amp);
    }

    #[test]
    fn routing_none_is_noop() {
        let mut s = ipc::Shared::default();
        let mut pv = empty_pv();
        let before = pv.scale_amp;
        apply_mod(&mut s, &mut pv, 0, 1.0);
        assert_eq!(pv.scale_amp, before);
    }

    #[test]
    fn routing_clamps_non_negative_controls() {
        let mut s = ipc::Shared::default();
        s.lighting[5] = 0.1; // glow
        let mut pv = empty_pv();
        // Target 2 = Glow, span 1.5; a big negative signal must clamp at 0.
        apply_mod(&mut s, &mut pv, 2, -1.0);
        assert_eq!(s.lighting[5], 0.0);
    }

    #[test]
    fn pulse_source_selects_beat_or_audio() {
        let mut s = ipc::Shared::default();
        s.audio[1] = 0.7; // bass envelope

        // Beat source (0): ignores audio, uses the decaying beat impulse — peaks
        // at 1.0 on the beat (beat_pos integral), decays away from it.
        s.pulse_source = 0;
        assert!((pulse_envelope(&s, 0.0) - 1.0).abs() < 1e-6);
        assert!(pulse_envelope(&s, 0.5) < 0.1);

        // Audio source (1): passes the bass band through (clamped), phase-agnostic.
        s.pulse_source = 1;
        assert_eq!(pulse_envelope(&s, 0.0), 0.7);
        assert_eq!(pulse_envelope(&s, 0.5), 0.7);
        s.audio[1] = 99.0;
        assert_eq!(pulse_envelope(&s, 0.0), 4.0); // clamped
    }

    #[test]
    fn audio_stereo_lean_follows_the_balance_and_lands_on_band_elements() {
        // #248 Tier 3. Off (drive off) → no lean; on → balance × stereo × Separation,
        // and the lean lands on the band-element X positions.
        let mut s = ipc::Shared::default();
        s.audio[7] = 1.0; // hard right
        assert_eq!(audio_stereo_lean(&s), 0.0, "lean is inert while the drive is off");
        s.audiodip[0] = 1.0; // drive on
        s.audiodip[6] = 1.0; // full stereo
        s.maxwell[4] = 3.0; // Separation
        assert!((audio_stereo_lean(&s) - 3.0).abs() < 1e-4, "hard right → +Separation");
        s.audio[7] = -0.5;
        assert!((audio_stereo_lean(&s) + 1.5).abs() < 1e-4, "half left → −0.5·Separation");
        // The lean shifts the band-multipole stack along X.
        s.audiodip[3] = 1.0; // multipole on
        s.audio[0] = 0.6; // some bass so a band element exists
        let elems = audio_band_elems(&s);
        assert!(!elems.is_empty());
        assert!(elems.iter().all(|e| e.pos.x < 0.0), "left lean pushes the stack to −X");
    }

    #[test]
    fn audio_dipole_drive_is_inert_off_and_follows_the_envelope_on() {
        // #248 Tier 1. Off (the default) → exactly 1, whatever the audio says:
        // the Maxwell field is byte-identical to pre-#248.
        let mut s = ipc::Shared::default();
        s.audio[5] = 0.8; // loud RMS
        assert_eq!(audio_dipole_drive(&s), 1.0);

        // On: drive = floor + amount·RMS.
        s.audiodip[0] = 1.0; // drive on
        s.audiodip[1] = 1.0; // amount
        s.audiodip[2] = 0.1; // floor
        assert!((audio_dipole_drive(&s) - 0.9).abs() < 1e-6);

        // Silence decays to the floor (the dim idle ember)…
        s.audio[5] = 0.0;
        assert!((audio_dipole_drive(&s) - 0.1).abs() < 1e-6);
        // …and floor 0 lets the field go fully dark between notes.
        s.audiodip[2] = 0.0;
        assert_eq!(audio_dipole_drive(&s), 0.0);

        // A blasting signal clamps to a sane ceiling (no NaN/∞ into the field).
        s.audio[5] = 1.0e6;
        assert_eq!(audio_dipole_drive(&s), 4.0);
        // A junk negative RMS (torn read) is treated as silence, not a sign flip.
        s.audio[5] = -3.0;
        assert_eq!(audio_dipole_drive(&s), 0.0);
    }

    #[test]
    fn audio_multipole_needs_both_gates_and_bands_follow_their_envelopes() {
        // #248 Tier 2. Multipole mode needs the audio drive AND the multipole
        // toggle; the per-band drives apply the Tier-1 mapping per envelope.
        let mut s = ipc::Shared::default();
        assert!(!audio_multipole_on(&s));
        s.audiodip[3] = 1.0; // multipole alone isn't enough — drive must be on
        assert!(!audio_multipole_on(&s));
        s.audiodip[0] = 1.0;
        assert!(audio_multipole_on(&s));

        s.audiodip[1] = 2.0; // amount
        s.audiodip[2] = 0.1; // floor
        s.audio[0] = 0.3; // sub
        s.audio[4] = 0.05; // high
        let d = audio_band_drives(&s);
        assert!((d[0] - 0.7).abs() < 1e-6, "sub: floor + 2·0.3");
        assert!((d[1] - 0.1).abs() < 1e-6, "silent band decays to the floor");
        assert!((d[4] - 0.2).abs() < 1e-6);

        // The element builder wires the generator's dials through: silence with
        // floor 0 → no sources at all (the field goes dark).
        s.audiodip[2] = 0.0;
        s.audio = [0.0; 8];
        assert!(audio_band_elems(&s).is_empty());
        // A driven band builds its multipole from the Maxwell separation/k dials.
        s.audio[2] = 0.5;
        let elems = audio_band_elems(&s);
        assert!(!elems.is_empty());
        assert!(elems.iter().all(|e| e.band == 2));
    }

    #[test]
    fn speed_pulse_bounces_a_decade_then_decays() {
        let mut b = 0.0;
        let dt = 1.0 / 60.0;
        // amount = 1 decade, fast attack, slow decay.
        let mut m = 1.0;
        for _ in 0..40 {
            m = speed_pulse_mult(&mut b, 1.0, 1.0, 0.005, 0.5, dt);
        }
        assert!(m > 8.0, "did not bounce ~×10: {m}");
        // Drive drops to 0 → multiplier decays back toward ×1.
        for _ in 0..240 {
            m = speed_pulse_mult(&mut b, 0.0, 1.0, 0.005, 0.5, dt);
        }
        assert!(m < 1.5, "did not decay back: {m}");
    }

    #[test]
    fn speed_pulse_inert_at_zero_amount() {
        let mut b = 0.0;
        // Even with full drive, amount 0 → ×1 (no effect).
        let m = speed_pulse_mult(&mut b, 1.0, 0.0, 0.005, 0.5, 1.0 / 60.0);
        assert!((m - 1.0).abs() < 1.0e-9, "amount 0 must be inert: {m}");
    }

    #[test]
    fn breath_inert_at_zero_amount() {
        let mut b = 0.0;
        // Full drive but amount 0 → unit scale (no breath).
        let s = breath_scale_vec(&mut b, 1.0, 0.0, 0.005, 0.5, 1.0 / 60.0);
        assert!((s - Vec3::ONE).length() < 1.0e-6, "amount 0 must be inert: {s:?}");
    }

    #[test]
    fn breath_swells_uniformly_to_amount() {
        let mut b = 0.0;
        // Settle the envelope to ~full drive; amount 2 → ≈ ×3 on every axis.
        let mut s = Vec3::ONE;
        for _ in 0..2000 {
            s = breath_scale_vec(&mut b, 1.0, 2.0, 0.005, 0.5, 1.0 / 60.0);
        }
        assert!((s.x - s.y).abs() < 1.0e-5 && (s.y - s.z).abs() < 1.0e-5,
            "breath must scale all axes equally: {s:?}");
        assert!((s.x - 3.0).abs() < 1.0e-3, "amount 2 → ×3 at full drive: {s:?}");
    }

    #[test]
    fn wind_velocity_is_constant_at_zero_depth() {
        use std::f64::consts::TAU;
        // depth 0 → velocity 1 everywhere, for every waveform (constant spin).
        for f in [FuncName::Sin, FuncName::Square, FuncName::Saw, FuncName::Triangle] {
            for k in 0..16 {
                let v = wind_velocity(f, k as f64 * TAU / 16.0, 0.0);
                assert!((v - 1.0).abs() < 1.0e-12, "{f:?} not constant at depth 0: {v}");
            }
        }
    }

    #[test]
    fn wind_velocity_shapes_but_never_reverses() {
        use std::f64::consts::TAU;
        // Sine, depth 0.5: peak speed at the crest, trough at the dip, mean ~1.
        assert!((wind_velocity(FuncName::Sin, 0.25 * TAU, 0.5) - 1.5).abs() < 1e-9);
        assert!((wind_velocity(FuncName::Sin, 0.75 * TAU, 0.5) - 0.5).abs() < 1e-9);
        // At full depth the trough touches 0 (a stall) but never goes negative.
        assert!(wind_velocity(FuncName::Sin, 0.75 * TAU, 1.0) >= 0.0);
        // Exotic unbounded funcs (tan) are clamped, so the spin can't blow up.
        let near_pole = wind_velocity(FuncName::Tan, 0.2499 * TAU, 1.0);
        assert!((0.0..=8.0).contains(&near_pole), "tan not clamped: {near_pole}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// organon#217 T3 — PBR text look controls: the three pure readers of the chain's
// tail (`Shared.glyph`, `Shared.glyph_cam`) and the uniform patch. Pure so every
// link can be pinned without a GPU, and so dropping one (`to_shared` for a slot, a
// packer line, a field here) is named by a test rather than discovered on screen.
// ─────────────────────────────────────────────────────────────────────────────

/// The glyph look off the param chain. Slot order is `Shared.glyph`'s contract
/// (`ipc.rs`); on a default snapshot this is exactly `GlyphLook::DEFAULT`, which is
/// what makes T3 inert for a preset saved before it (invariant #4). Bevel and crown
/// (`[11]`, `[12]`) are not `GlyphLook` fields — they ride `Uniforms.shape` through
/// [`glyph_shape`] — so they are not read here.
fn glyph_look_from(s: &ipc::Shared) -> glyph_ring::GlyphLook {
    let g = &s.glyph;
    glyph_ring::GlyphLook {
        cell_w: g[0].max(1e-3),
        depth: g[1].max(0.0),
        gap: g[2].max(0.0),
        gain: g[3].max(0.0),
        faceplate: [g[4], g[4], g[4]],
        backplane: [g[5], g[6], g[7]],
        margin: g[8].max(0.0),
        backplane_depth: g[9].max(0.0),
        default_fg: [g[10], g[10], g[10]],
    }
}

/// `Uniforms.shape` for this frame: with a glyph ring drawing, `x` is the glyph look's
/// own bevel, `y` its face crown and `z` T9's emission-profile strength
/// (`Shared.glyph[11]`, `[12]`, `[13]` — `cube.wgsl::tile_profile` reads `shape.z`, and
/// is exactly 1.0 at 0, so a default lane is T1's even glow); with no ring, the frame's
/// own lanes, untouched — byte-identical to before T3.
fn glyph_shape(frame: [f32; 4], glyph_live: bool, glyph: &[f32; 16]) -> [f32; 4] {
    if glyph_live {
        [glyph[11].clamp(0.0, 1.0), glyph[12].clamp(0.0, 1.0), glyph[13].clamp(0.0, 1.0), frame[3]]
    } else {
        frame
    }
}

/// The lowering options off the param chain (organon#217 T9): `Shared.glyph[14]` is
/// the dark-tile switch, a flag on an `f32` lane read as `> 0.5` the way the held
/// camera's `glyph_cam[0]` is. A default snapshot yields `LowerOptions::default()`,
/// under which `lower_grid_with` is `lower_grid` byte for byte — so a preset saved before
/// the switch lowers the grid it lowered yesterday (invariant #4). ⚠️ `..Default::default()`
/// on purpose: `LowerOptions` is a struct precisely so the next lowering-only switch is a
/// field, not a signature (T12's `motion`, proposed on `glyph[15]`, is in flight in
/// another branch), and a bare literal here would leave `main` not compiling the moment
/// both land while each was green alone. A new field arrives at its inert default until
/// its own one-line wire is added below.
fn glyph_lower_options(glyph: &[f32; 16]) -> glyph_ring::LowerOptions {
    glyph_ring::LowerOptions { dark_tiles: glyph[14] > 0.5, ..Default::default() }
}

/// The distance at which a box of `half_w × half_h` (world units, facing the camera)
/// exactly fills a frame of vertical FOV `fov_deg` and aspect `aspect` (w / h):
/// the larger of the vertical fit and the horizontal fit. ⚠️ Computed from the bounds
/// and the FOV, never sized by feel from the wheel — a notch on the visual is
/// `distance *= 1 − dy·0.001`, which is no unit at all.
fn fit_distance(half_w: f32, half_h: f32, fov_deg: f32, aspect: f32) -> f32 {
    let t = (fov_deg.clamp(4.0, 120.0).to_radians() * 0.5).tan().max(1e-4);
    let a = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    (half_h / t).max(half_w / (t * a))
}

/// The held camera for a live glyph ring (`doc/pbr_text_engine.md` §5.1 / §8), as the
/// same absolute tuple the substrate rig uses — `(centre, yaw, pitch, distance, roll,
/// fov_deg)` — or `None` when it does not apply: no ring drawing, or `glyph_cam[0]`
/// (hold) off. Yaw 0 looks down −z at the grid's front (`+z` is toward the camera in
/// `cell_centre`'s frame); pitch is `glyph_cam[1]` in degrees, clamped to the orbit's
/// own `PITCH_LIMIT`; distance is [`fit_distance`] over the bounds' `x`/`y` extent
/// times `glyph_cam[2]` (zoom), clamped to the viewpoint's `DISTANCE_MIN..=MAX`; roll
/// 0; the FOV is the frame's own, so a preset's `cam_fov` still applies. The tiles'
/// bounds already include the backplane margin, so "fills the frame" means the
/// backplane does. Empty or non-finite bounds (nothing drawn yet) → `None`, so the
/// orbit rig keeps the camera rather than a NaN taking it.
fn glyph_camera_rig(
    glyph_live: bool,
    cam: &[f32; 8],
    bounds: &math::Bounds,
    size: (u32, u32),
    fov_deg: f32,
) -> Option<(Vec3, f32, f32, f32, f32, f32)> {
    if !glyph_live || cam[0] < 0.5 {
        return None;
    }
    if !(bounds.min.is_finite() && bounds.max.is_finite()) || bounds.max.x <= bounds.min.x || bounds.max.y <= bounds.min.y {
        return None;
    }
    let centre = bounds.center();
    let half_w = (bounds.max.x - bounds.min.x) * 0.5;
    let half_h = (bounds.max.y - bounds.min.y) * 0.5;
    let aspect = if size.1 > 0 { size.0 as f32 / size.1 as f32 } else { 1.0 };
    let zoom = if cam[2].is_finite() && cam[2] > 0.0 { cam[2] } else { 1.0 };
    let distance = (fit_distance(half_w, half_h, fov_deg, aspect) * zoom)
        .clamp(scene_input::DISTANCE_MIN, scene_input::DISTANCE_MAX);
    let pitch = cam[1].to_radians().clamp(-scene_input::PITCH_LIMIT, scene_input::PITCH_LIMIT);
    Some((centre, 0.0, pitch, distance, 0.0, fov_deg))
}

// ─────────────────────────────────────────────────────────────────────────────
// organon#217 T10 — glyphs as lights (`doc/pbr_text_engine.md` §4.1, §15).
//
// The emissive-cubes-as-lights path (#167 T3) is the renderer's: `gi.rs::update_lights`
// takes `Surface.meta_nodes`, ranks them by luminance, and uploads the brightest N as
// point lights `cube.wgsl::many_lights` loops. The world owns what goes INTO that node
// set, and with a glyph ring live the answer used to be wrong in a way nothing reported:
// the tiles had replaced the generator's instances, so the node builder handed the
// renderer every tile — the backplane included — coloured by its TINT (the near-black
// faceplate) or, with no palette, by its POSITION in the bounds, so the "brightest"
// cells were the ones nearest the grid's top-right corner. The functions below lower
// the grid's EMISSION into that set instead: a lit tile becomes a candidate at its
// front face carrying `emit.rgb * emit.w` — the exact value the shader adds to
// `emissive` — so the pool a glyph throws onto the backplane is the colour the glyph
// shows. Pure, so the selection is pinned without a GPU; with no ring the node builder
// takes the branch it always took, byte for byte (invariant #4).
// ─────────────────────────────────────────────────────────────────────────────

/// A point-light candidate lowered from the glyph grid. `pos` is on the tile's FRONT
/// face — the phosphor's own surface, where the emission leaves — so the light shines
/// past the tile's edges onto the backplane behind it and never lights the face it sits
/// on (a light in the face's plane has `n·l = 0` there). `radiance` is linear,
/// `emit.rgb * emit.w`: SDR-white units, the same the surface emits in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GlyphLight {
    pos: Vec3,
    radiance: Vec3,
}

/// Rec. 709 luminance of a LINEAR colour — the weights `gi.rs::update_lights` ranks by,
/// so the world's pre-selection and the renderer's own cannot disagree about which
/// candidate is brighter. ⚠️ Linear, never sRGB-encoded: the encode compresses the top
/// of every channel, so under it a mid grey out-ranks a saturated green it is dimmer
/// than (`lights_are_ranked_by_linear_luminance_not_srgb` pins the pair).
fn linear_luminance(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// How many adjacent lit tiles of one row fold into ONE light, and how far apart (in
/// column widths) two tiles may sit and still count as adjacent. A glyph stroke is a
/// line of tiles, and sixteen thousand cells cannot each be a light: with the cap at 64
/// an unclustered logo lights only its 64 brightest cells, and a stroke lit at one end
/// reads as a stroke with a bulb in it. A folded run is one light at the luminance-
/// weighted centroid carrying the SUM of its members' radiance — exact in the far
/// field, and within the pool radius the ends of a run of four are 1.5 cells from its
/// centre, well inside the falloff. Four rather than a whole stroke because the pool
/// under a long run would otherwise peak at its middle and fade at its ends. The gap
/// admits half-width tiles (`▌` then `▐` in the next cell are 1.5 widths apart) and
/// refuses one empty cell (2.0).
const GLYPH_LIGHT_RUN: usize = 4;
const GLYPH_LIGHT_GAP: f32 = 1.6;

/// Pure: the light candidates for a lowered grid — one per RUN of adjacent lit tiles
/// in a row (see [`GLYPH_LIGHT_RUN`]), at the run's luminance-weighted centroid on the
/// tiles' front faces, carrying the run's summed linear radiance. `instances` /
/// `emits` are the parallel buffers `lower_grid` filled; a length mismatch is the
/// renderer's own "no emission" convention and yields nothing, as does a grid whose
/// every cell is dark — an unlit terminal sheds no light, and the backplane (emission
/// zero) is never a candidate. `rows` decides which cell row a tile's `y` falls in:
/// cell centres sit at half-integer row pitches when `rows` is even and at integers
/// when it is odd, and a sub-cell tile (`▄`, `▁`) is offset from its centre by up to
/// 3/8 of a row, so rounding `y / cell_h` would split one row's tiles two ways.
fn glyph_light_candidates(instances: &[Mat4], emits: &[Vec4], cell_w: f32, cell_h: f32, rows: usize) -> Vec<GlyphLight> {
    if instances.len() != emits.len() || !(cell_w > 0.0) || !(cell_h > 0.0) {
        return Vec::new();
    }
    let row_phase = if rows % 2 == 0 { 0.0 } else { 0.5 };
    let row_of = |y: f32| (y / cell_h + row_phase).floor() as i64;
    let mut lit: Vec<(i64, GlyphLight)> = instances
        .iter()
        .zip(emits)
        .filter_map(|(m, e)| {
            let radiance = Vec3::new(e.x, e.y, e.z) * e.w;
            if !(linear_luminance(radiance) > 0.0) {
                return None;
            }
            // `lower_grid` builds every tile as scale·identity·translate, so the front
            // face is the translation plus half the z extent along +z.
            let centre = m.w_axis.truncate();
            let depth = m.z_axis.truncate().length();
            let pos = centre + Vec3::new(0.0, 0.0, depth * 0.5);
            Some((row_of(pos.y), GlyphLight { pos, radiance }))
        })
        .collect();
    lit.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.pos.x.partial_cmp(&b.1.pos.x).unwrap_or(std::cmp::Ordering::Equal)));
    let gap = GLYPH_LIGHT_GAP * cell_w;
    let mut out = Vec::with_capacity(lit.len());
    let mut i = 0;
    while i < lit.len() {
        let row = lit[i].0;
        let mut j = i + 1;
        while j < lit.len() && j - i < GLYPH_LIGHT_RUN && lit[j].0 == row && lit[j].1.pos.x - lit[j - 1].1.pos.x <= gap {
            j += 1;
        }
        let mut radiance = Vec3::ZERO;
        let mut weighted = Vec3::ZERO;
        let mut weight = 0.0f32;
        for (_, l) in &lit[i..j] {
            let lum = linear_luminance(l.radiance);
            radiance += l.radiance;
            weighted += l.pos * lum;
            weight += lum;
        }
        out.push(GlyphLight { pos: weighted / weight, radiance });
        i = j;
    }
    out
}

/// Pure: the brightest `n` candidates by LINEAR luminance (a partial select — order
/// within the chosen set is the renderer's business). `n == 0` is no light; fewer
/// candidates than `n` is all of them. The renderer's `update_lights` ranks the node set
/// by the same weights, so handing it exactly `n` makes its own select the identity;
/// under ReSTIR (`restir[0]`) the caller hands it every candidate instead, because
/// reservoir sampling wants the dim ones as a pool to rotate through.
fn brightest_glyph_lights(mut cands: Vec<GlyphLight>, n: usize) -> Vec<GlyphLight> {
    if n == 0 {
        cands.clear();
    } else if cands.len() > n {
        cands.select_nth_unstable_by(n - 1, |a, b| {
            linear_luminance(b.radiance).partial_cmp(&linear_luminance(a.radiance)).unwrap_or(std::cmp::Ordering::Equal)
        });
        cands.truncate(n);
    }
    cands
}

/// Pure: the many-lights radius lane (`manylight[2]`) as the renderer wants it — a
/// fraction of the light bounds' diagonal — **re-denominated in column widths while a
/// ring is live.** §5.1's rule ("express depth in cell units, never pixels") holds for
/// every glyph length, and a pool radius is one: as a fraction of the scene diagonal
/// the same lane is 2.6 cells on the 81×10 logo and 6.8 on a 200×50 fullscreen grid,
/// so the pool a glyph throws would grow with the amount of text on screen. With no
/// ring the lane is returned untouched, so a generator frame is byte-identical
/// (invariant #4); a degenerate diagonal (nothing lowered yet) leaves it untouched too.
/// `light_min` / `light_max` must be the SAME bounds the renderer multiplies the
/// fraction back by (`gi_min` / `gi_max`), or the round trip is not one.
fn glyph_light_radius_frac(lane: f32, glyph_live: bool, cell_w: f32, light_min: Vec3, light_max: Vec3) -> f32 {
    if !glyph_live {
        return lane;
    }
    let diag = (light_max - light_min).length();
    if !(diag.is_finite() && diag > 1e-6 && cell_w > 0.0) {
        return lane;
    }
    lane.max(0.0) * cell_w / diag
}

/// Pure: the colour a Plexus node carries for one instance of the cloud it is built from
/// (organon#217 W17). With no glyph ring the node keeps the generator's tint — every
/// field the web has ever wired, byte for byte (invariant #4). **While a ring is live the
/// cloud is the lowered grid, and the node's colour is the tile's EMISSION**: `emit.rgb ×
/// emit.w`, the linear radiance `cube.wgsl` adds past the albedo and the light lowering
/// (`glyph_light_candidates`) ranks by — never the faceplate tint, which is the near-black
/// dielectric in FRONT of the phosphor (§4) and reads as the same grey on every tile, so
/// no hue lane could recolour it (`apply_hsv` on a grey is a grey). It feeds `ntints`
/// because that is the only lane there is: the plexus impostor (`ArmInstance::color`)
/// carries one colour and `fs_capsule` derives BOTH its albedo and, × the material's
/// glow, its emission from it — the T6 core shows exactly that emission through the
/// shell — and Tier 1's markers ride `tints` the same way. A dark tile (`emit.rgb == 0`)
/// is a dark node, not a faceplate-grey one; the backplane (emission zero) likewise. `w`
/// is kept from the tint: neither tier reads it, and `lower_grid` writes 1.
fn plexus_node_colour(glyph_live: bool, tint: Vec4, emit: Vec4) -> Vec4 {
    if !glyph_live {
        return tint;
    }
    Vec4::new(emit.x * emit.w, emit.y * emit.w, emit.z * emit.w, tint.w)
}

#[cfg(test)]
mod plexus_glyph_tests {
    use super::*;

    /// `GlyphLook::DEFAULT.faceplate`, as `lower_grid` tints every tile.
    fn faceplate() -> Vec4 {
        Vec4::new(0.03, 0.03, 0.03, 1.0)
    }

    /// A lit tile's node is its emission — the linear radiance `emit.rgb × emit.w`, the
    /// term `cube.wgsl` adds — and not the faceplate the tile was tinted with.
    #[test]
    fn a_lit_tile_makes_its_node_the_emission_not_the_faceplate() {
        let emit = Vec4::new(0.0, 0.8, 0.1, 3.0);
        let got = plexus_node_colour(true, faceplate(), emit);
        assert_eq!(got.truncate(), Vec3::new(0.0, 2.4, 0.3), "a lit tile's node must carry the tile's emission scaled by its gain, not the faceplate");
        assert_ne!(got.truncate(), faceplate().truncate(), "a lit tile's node must not be the faceplate grey");
    }

    /// A dark tile — or the backplane, whose emission is zero — makes a dark node, not a
    /// faceplate-grey (or backplane-grey) one.
    #[test]
    fn a_dark_tile_makes_a_dark_node_not_a_faceplate_grey_one() {
        let dark = plexus_node_colour(true, faceplate(), Vec4::new(0.0, 0.0, 0.0, 3.0));
        assert_eq!(dark.truncate(), Vec3::ZERO, "a dark tile's node must be dark");
        let backplane = plexus_node_colour(true, Vec4::new(0.06, 0.06, 0.065, 1.0), Vec4::ZERO);
        assert_eq!(backplane.truncate(), Vec3::ZERO, "the backplane node must be dark");
    }

    /// Invariant #4: with no ring the generator's tint is the node's colour, whatever
    /// sits in the emit lane.
    #[test]
    fn with_no_ring_the_node_keeps_the_generator_tint() {
        let tint = Vec4::new(0.9, 0.2, 0.4, 1.0);
        assert_eq!(plexus_node_colour(false, tint, Vec4::new(1.0, 1.0, 1.0, 5.0)), tint);
        assert_eq!(plexus_node_colour(false, tint, Vec4::ZERO), tint);
    }
}

#[cfg(test)]
mod glyph_light_tests {
    use super::*;
    use glam::Quat;

    /// A tile as `lower_grid` builds it: `size` × identity × `pos`.
    fn tile(pos: (f32, f32, f32), size: (f32, f32, f32)) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::new(size.0, size.1, size.2),
            Quat::IDENTITY,
            Vec3::new(pos.0, pos.1, pos.2),
        )
    }

    /// A full-block tile at column `c` of a one-row grid (cell 1×2, depth 0.18), with
    /// `emit` = (rgb, gain).
    fn block(c: f32, y: f32, emit: [f32; 4]) -> (Mat4, Vec4) {
        (tile((c, y, 0.09), (1.0, 2.0, 0.18)), Vec4::new(emit[0], emit[1], emit[2], emit[3]))
    }

    fn split(v: Vec<(Mat4, Vec4)>) -> (Vec<Mat4>, Vec<Vec4>) {
        v.into_iter().unzip()
    }

    /// The pair the encode flips: a saturated green whose LINEAR luminance (0.2146)
    /// beats a mid grey's (0.2), and whose sRGB-encoded luminance (0.418) loses to the
    /// grey's (0.484). Ranking by the encoded value picks the grey.
    #[test]
    fn lights_are_ranked_by_linear_luminance_not_srgb() {
        let green = GlyphLight { pos: Vec3::ZERO, radiance: Vec3::new(0.0, 0.3, 0.0) };
        let grey = GlyphLight { pos: Vec3::X * 5.0, radiance: Vec3::splat(0.2) };
        assert!(linear_luminance(green.radiance) > linear_luminance(grey.radiance));
        let chosen = brightest_glyph_lights(vec![grey, green], 1);
        assert_eq!(
            chosen,
            vec![green],
            "brightest-1 must be chosen by LINEAR luminance (green 0.2146 > grey 0.2); an sRGB-encoded rank picks the grey"
        );
        // And the count is honoured both ways: 0 is no light, more than there are is all.
        assert!(brightest_glyph_lights(vec![grey, green], 0).is_empty());
        assert_eq!(brightest_glyph_lights(vec![grey, green], 5).len(), 2);
    }

    /// Emission is `rgb * gain` in linear light — a brighter GAIN on a dimmer colour can
    /// out-rank a brighter colour at unit gain, and the candidate carries the product.
    #[test]
    fn a_candidate_carries_rgb_times_gain() {
        let (i, e) = split(vec![block(0.0, 0.0, [0.2, 0.2, 0.2, 3.0]), block(2.0, 0.0, [0.0, 0.5, 0.0, 1.0])]);
        let c = glyph_light_candidates(&i, &e, 1.0, 2.0, 1);
        assert_eq!(c.len(), 2, "one cell apart is a gap, not a run");
        let chosen = brightest_glyph_lights(c, 1);
        assert!((chosen[0].radiance - Vec3::splat(0.6)).length() < 1e-6, "grey at gain 3 = 0.6 linear, out-ranks green 0.5: {chosen:?}");
    }

    /// An all-dark grid sheds no light — and the backplane, which never emits, is never
    /// a candidate whatever its tint. A generator frame (no `emits`) yields nothing too.
    #[test]
    fn a_dark_grid_and_a_generator_frame_shed_no_light() {
        let (i, e) = split(vec![block(0.0, 0.0, [0.0; 4]), block(1.0, 0.0, [1.0, 1.0, 1.0, 0.0])]);
        assert!(glyph_light_candidates(&i, &e, 1.0, 2.0, 1).is_empty(), "zero rgb, or zero gain, is dark");
        // The backplane: a big slab with a visible tint and `Vec4::ZERO` emission.
        let (i, e) = split(vec![(tile((0.0, 0.0, -0.2), (84.0, 23.0, 0.25)), Vec4::ZERO)]);
        assert!(glyph_light_candidates(&i, &e, 1.0, 2.0, 1).is_empty());
        // A generator's instances arrive with an EMPTY emit buffer: nothing to lower.
        let (i, _) = split(vec![block(0.0, 0.0, [1.0; 4])]);
        assert!(glyph_light_candidates(&i, &[], 1.0, 2.0, 1).is_empty(), "length mismatch is the renderer's 'no emission'");
    }

    /// The light sits on the tile's FRONT face: the translation plus half the z extent.
    #[test]
    fn the_light_sits_on_the_front_face() {
        let (i, e) = split(vec![block(0.0, 0.0, [0.0, 1.0, 0.0, 1.0])]);
        let c = glyph_light_candidates(&i, &e, 1.0, 2.0, 1);
        assert_eq!(c.len(), 1);
        assert!((c[0].pos.z - 0.18).abs() < 1e-6, "front face of a 0.18-deep tile at z 0.09 is z 0.18: {}", c[0].pos.z);
        assert_eq!(c[0].pos.x, 0.0);
    }

    /// A horizontal stroke of three lit tiles folds into ONE light at its centroid
    /// carrying three tiles' radiance; a run longer than `GLYPH_LIGHT_RUN` splits; a
    /// one-cell gap splits; a second row is a second light.
    #[test]
    fn a_horizontal_stroke_folds_into_one_light_at_its_centroid() {
        let g = [0.0, 1.0, 0.0, 1.0];
        let (i, e) = split(vec![block(0.0, 0.0, g), block(1.0, 0.0, g), block(2.0, 0.0, g)]);
        let c = glyph_light_candidates(&i, &e, 1.0, 2.0, 1);
        assert_eq!(c.len(), 1, "three adjacent tiles → one light: {c:?}");
        assert!((c[0].pos.x - 1.0).abs() < 1e-6, "at the centroid, the middle tile: {}", c[0].pos.x);
        assert!((c[0].radiance - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-6, "summed radiance: {:?}", c[0].radiance);
        // Order of arrival does not matter — the fold sorts by row then x.
        let (i, e) = split(vec![block(2.0, 0.0, g), block(0.0, 0.0, g), block(1.0, 0.0, g)]);
        assert_eq!(glyph_light_candidates(&i, &e, 1.0, 2.0, 1).len(), 1);
        // A run of five: four and one.
        let (i, e) = split((0..5).map(|c| block(c as f32, 0.0, g)).collect());
        let c = glyph_light_candidates(&i, &e, 1.0, 2.0, 1);
        assert_eq!(c.len(), 2, "GLYPH_LIGHT_RUN caps a run at {GLYPH_LIGHT_RUN}: {c:?}");
        // A gap of one empty cell breaks the run.
        let (i, e) = split(vec![block(0.0, 0.0, g), block(2.0, 0.0, g)]);
        assert_eq!(glyph_light_candidates(&i, &e, 1.0, 2.0, 1).len(), 2);
        // Two rows, one tile each, same x: two lights (a vertical stroke does not fold).
        let (i, e) = split(vec![block(0.0, 1.0, g), block(0.0, -1.0, g)]);
        assert_eq!(glyph_light_candidates(&i, &e, 1.0, 2.0, 2).len(), 2);
    }

    /// The row key must not split one row's tiles: on an even-row grid a full block at
    /// `y = 4.5·h` and a lower half block at `y = 4.0·h` are the same row (adjacent
    /// columns → one light); on an odd-row grid the centres are integer pitches and a
    /// `▄` sits at `r − 0.25`. Rounding `y / h` would put the two halves in different rows.
    #[test]
    fn the_row_key_keeps_sub_cell_tiles_in_their_row() {
        let g = [1.0, 1.0, 1.0, 1.0];
        // rows = 10: row 0's centre is 4.5·h = 9.0; a lower half block in the next column is at 8.5.
        let full = (tile((0.0, 9.0, 0.09), (1.0, 2.0, 0.18)), Vec4::new(1.0, 1.0, 1.0, 1.0));
        let lower = (tile((1.0, 8.5, 0.09), (1.0, 1.0, 0.18)), Vec4::new(g[0], g[1], g[2], g[3]));
        let (i, e) = split(vec![full, lower]);
        assert_eq!(glyph_light_candidates(&i, &e, 1.0, 2.0, 10).len(), 1, "same row, adjacent → one light");
        // rows = 3: row 1's centre is 0; a lower half block beside it is at −0.5.
        let (i, e) = split(vec![block(0.0, 0.0, g), (tile((1.0, -0.5, 0.09), (1.0, 1.0, 0.18)), Vec4::new(1.0, 1.0, 1.0, 1.0))]);
        assert_eq!(glyph_light_candidates(&i, &e, 1.0, 2.0, 3).len(), 1);
    }

    /// The radius lane is in CELLS while a ring is live and untouched otherwise: with
    /// bounds whose diagonal is 50 world units and a 1-unit column, a lane of 2.0 (two
    /// columns) becomes the fraction 0.04 — which the renderer multiplies by the same
    /// diagonal back to 2.0 world units.
    #[test]
    fn the_radius_is_in_cells_while_live_and_the_lane_otherwise() {
        let (lo, hi) = (Vec3::new(-15.0, -20.0, 0.0), Vec3::new(15.0, 20.0, 0.0));
        assert_eq!(glyph_light_radius_frac(0.5, false, 1.0, lo, hi), 0.5, "no ring: the lane, untouched");
        let frac = glyph_light_radius_frac(2.0, true, 1.0, lo, hi);
        assert!((frac - 0.04).abs() < 1e-6, "{frac}");
        assert!((frac * (hi - lo).length() - 2.0).abs() < 1e-5, "the round trip through the renderer's diagonal is two columns");
        assert_eq!(glyph_light_radius_frac(2.0, true, 1.0, lo, lo), 2.0, "no diagonal yet: the lane, untouched");
        assert_eq!(glyph_light_radius_frac(-1.0, true, 1.0, lo, hi), 0.0, "a negative lane is no radius, not a negative one");
    }
}

#[cfg(test)]
mod glyph_look_tests {
    use super::*;

    fn bounds(min: (f32, f32, f32), max: (f32, f32, f32)) -> math::Bounds {
        math::Bounds { min: Vec3::new(min.0, min.1, min.2), max: Vec3::new(max.0, max.1, max.2) }
    }

    /// Invariant #4, stated as bytes: a default `Shared` reads back as T1's one const,
    /// field for field. Every default in `ipc::Shared::default().glyph` is pinned here
    /// against `GlyphLook::DEFAULT`, so neither can drift from the other unnoticed.
    #[test]
    fn a_default_snapshot_is_exactly_the_t1_look() {
        let s = ipc::Shared::default();
        let got = glyph_look_from(&s);
        let want = glyph_ring::GlyphLook::DEFAULT;
        assert_eq!(got.cell_w, want.cell_w);
        assert_eq!(got.depth, want.depth);
        assert_eq!(got.gap, want.gap);
        assert_eq!(got.gain, want.gain);
        assert_eq!(got.faceplate, want.faceplate);
        assert_eq!(got.backplane, want.backplane);
        assert_eq!(got.margin, want.margin);
        assert_eq!(got.backplane_depth, want.backplane_depth);
        assert_eq!(got.default_fg, want.default_fg);
        // And the two lanes that ride the uniform rather than the look: T1 drew a sharp,
        // flat tile (it rode `Shared.bevel`, default 0).
        assert_eq!(s.glyph[11], 0.0, "bevel default must be T1's sharp tile");
        assert_eq!(s.glyph[12], 0.0, "crown default must be flat");
        // organon#217 T9 — and the two lanes the tile added: a flat emission profile
        // (`tile_profile` is exactly 1.0 at 0) and only lit cells tiled.
        assert_eq!(s.glyph[13], 0.0, "profile default must be flat (T1's even glow)");
        assert_eq!(s.glyph[14], 0.0, "dark tiles default off (only lit cells get tiles)");
        assert_eq!(
            glyph_shape([0.5, 0.0, 0.0, 7.0], true, &s.glyph),
            [0.0, 0.0, 0.0, 7.0],
            "a live ring on a default snapshot writes shape.z = 0, the inert profile"
        );
        assert_eq!(
            glyph_lower_options(&s.glyph),
            glyph_ring::LowerOptions::default(),
            "a default snapshot lowers exactly as `lower_grid` does"
        );
    }

    /// organon#217 T9 — the two wires, each a pure twin of the world's read so dropping
    /// either is a named failure rather than a tile that looks wrong: `glyph[13]` rides
    /// `Uniforms.shape.z` clamped to 0..1, and `glyph[14]` is a flag read as `> 0.5`
    /// into `LowerOptions::dark_tiles`.
    #[test]
    fn the_profile_lane_rides_shape_z_and_the_dark_tile_lane_is_a_flag() {
        let mut s = ipc::Shared::default();
        s.glyph[13] = 0.5;
        assert_eq!(
            glyph_shape([0.0, 0.0, 0.9, 0.0], true, &s.glyph)[2],
            0.5,
            "shape.z must be glyph[13] while a ring is live — the profile wire is missing"
        );
        s.glyph[13] = 3.0;
        assert_eq!(glyph_shape([0.0; 4], true, &s.glyph)[2], 1.0, "the profile clamps to 1");
        s.glyph[13] = -1.0;
        assert_eq!(glyph_shape([0.0; 4], true, &s.glyph)[2], 0.0, "and to 0");
        assert_eq!(glyph_shape([0.0, 0.0, 0.9, 0.0], false, &s.glyph)[2], 0.9, "no ring: untouched");

        s.glyph[14] = 1.0;
        assert!(glyph_lower_options(&s.glyph).dark_tiles, "glyph[14] = 1 must switch dark tiles on — the wire is missing");
        s.glyph[14] = 0.5;
        assert!(!glyph_lower_options(&s.glyph).dark_tiles, "exactly 0.5 is off (`> 0.5`, the flag rule)");
        s.glyph[14] = 0.51;
        assert!(glyph_lower_options(&s.glyph).dark_tiles);
        s.glyph[14] = 0.0;
        assert!(!glyph_lower_options(&s.glyph).dark_tiles);
    }

    /// Every slot reaches the field the contract names — the "drop one link" test.
    /// Writing a distinct value into each slot and reading it back at the field it
    /// documents fails on a swapped, skipped or duplicated slot, which is the way a
    /// hand-maintained slot list goes wrong.
    #[test]
    fn every_look_slot_reaches_its_field() {
        let mut s = ipc::Shared::default();
        for (i, v) in s.glyph.iter_mut().enumerate() {
            *v = 10.0 + i as f32;
        }
        let l = glyph_look_from(&s);
        assert_eq!(l.cell_w, 10.0);
        assert_eq!(l.depth, 11.0);
        assert_eq!(l.gap, 12.0);
        assert_eq!(l.gain, 13.0);
        assert_eq!(l.faceplate, [14.0; 3]);
        assert_eq!(l.backplane, [15.0, 16.0, 17.0]);
        assert_eq!(l.margin, 18.0);
        assert_eq!(l.backplane_depth, 19.0);
        assert_eq!(l.default_fg, [20.0; 3]);
        assert_eq!(glyph_shape([0.0; 4], true, &s.glyph), [1.0, 1.0, 1.0, 0.0], "bevel/crown/profile clamp to 1");
        s.glyph[11] = 0.3;
        s.glyph[12] = 0.4;
        s.glyph[13] = 0.5;
        assert_eq!(glyph_shape([0.9, 0.0, 5.0, 6.0], true, &s.glyph), [0.3, 0.4, 0.5, 6.0]);
        assert!(glyph_lower_options(&s.glyph).dark_tiles, "slot 14 (24.0) reads as the switch on");
    }

    /// No ring → the frame's own `shape` lanes, untouched, whatever the look says.
    #[test]
    fn the_shape_patch_is_inert_without_a_ring() {
        let mut s = ipc::Shared::default();
        s.glyph[11] = 0.8;
        s.glyph[12] = 0.9;
        assert_eq!(glyph_shape([0.25, 0.0, 0.0, 0.0], false, &s.glyph), [0.25, 0.0, 0.0, 0.0]);
    }

    /// A degenerate look cannot reach `lower_grid`: cell width is floored above zero
    /// (a zero-width cell makes every tile a NaN transform) and the lengths are >= 0.
    #[test]
    fn a_degenerate_look_is_floored() {
        let mut s = ipc::Shared::default();
        s.glyph[0] = 0.0;
        s.glyph[1] = -1.0;
        let l = glyph_look_from(&s);
        assert!(l.cell_w > 0.0);
        assert_eq!(l.depth, 0.0);
    }

    /// `fit_distance` is the textbook frustum fit: at 90 deg the tangent is 1, so a box
    /// of half-height 10 fills a square frame at distance 10, and a wider box is limited
    /// by the horizontal fit (half-width over tan * aspect).
    #[test]
    fn the_fit_is_computed_from_the_bounds_and_the_fov() {
        let d = fit_distance(10.0, 10.0, 90.0, 1.0);
        assert!((d - 10.0).abs() < 1e-4, "{d}");
        // Wide box, square frame: the width governs.
        let d = fit_distance(30.0, 10.0, 90.0, 1.0);
        assert!((d - 30.0).abs() < 1e-4, "{d}");
        // Same wide box in a 3:1 frame: the width fits at 10 again, so the height governs.
        let d = fit_distance(30.0, 10.0, 90.0, 3.0);
        assert!((d - 10.0).abs() < 1e-4, "{d}");
        // A longer lens stands further back: 45 deg needs 1/tan(22.5 deg) ~ 2.414x.
        let d = fit_distance(10.0, 10.0, 45.0, 1.0);
        assert!((d - 10.0 / (22.5f32.to_radians()).tan()).abs() < 1e-3, "{d}");
    }

    /// The rig exists exactly when a ring is drawing AND the preset asked to hold —
    /// the two gates that keep a session with no ring, and a ring under a preset that
    /// did not ask, on the orbit rig they had before T3.
    #[test]
    fn the_rig_needs_both_a_live_ring_and_the_hold() {
        let b = bounds((-50.0, -30.0, -1.0), (50.0, 30.0, 0.2));
        let hold = [1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let off = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert!(glyph_camera_rig(true, &hold, &b, (1920, 1080), 45.0).is_some());
        assert!(glyph_camera_rig(false, &hold, &b, (1920, 1080), 45.0).is_none(), "no ring");
        assert!(glyph_camera_rig(true, &off, &b, (1920, 1080), 45.0).is_none(), "hold off");
        assert!(glyph_camera_rig(true, &hold, &math::Bounds::new(), (1920, 1080), 45.0).is_none(), "nothing drawn");
    }

    /// The rig looks straight down -z at the grid's centre from the fitted distance,
    /// tilts by `glyph_cam[1]` degrees, scales by `glyph_cam[2]`, keeps the frame's FOV,
    /// and rolls nothing. And it is a pure function of its inputs: the same frame twice
    /// gives the same tuple, which is the property the path tracer's `pt_moved` needs.
    #[test]
    fn the_rig_frames_the_grid_and_holds_still() {
        let b = bounds((-50.0, -30.0, -1.0), (50.0, 30.0, 0.2));
        let cam = [1.0, 6.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let rig = glyph_camera_rig(true, &cam, &b, (1920, 1080), 45.0).unwrap();
        let (centre, yaw, pitch, distance, roll, fov) = rig;
        assert_eq!(centre, b.center());
        assert_eq!(yaw, 0.0);
        assert!((pitch - 6.0f32.to_radians()).abs() < 1e-6);
        assert_eq!(roll, 0.0);
        assert_eq!(fov, 45.0);
        let want = fit_distance(50.0, 30.0, 45.0, 1920.0 / 1080.0);
        assert!((distance - want).abs() < 1e-3, "{distance} vs {want}");
        assert_eq!(glyph_camera_rig(true, &cam, &b, (1920, 1080), 45.0).unwrap(), rig, "held still");
        // Zoom scales the fitted distance; the tilt is clamped to the orbit's own limit.
        let cam2 = [1.0, 500.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let (_, _, p2, d2, _, _) = glyph_camera_rig(true, &cam2, &b, (1920, 1080), 45.0).unwrap();
        assert!((d2 - 2.0 * want).abs() < 1e-3);
        assert_eq!(p2, scene_input::PITCH_LIMIT);
    }

    /// The whole point of the hold, end to end with T5's pure pieces: with the rig
    /// fixed the unjittered view-proj is bit-identical frame to frame, so `pt_moved`
    /// is false and the dwell accumulates; with the orbit running it restarts every
    /// frame and never converges — which is what the first GPU look saw.
    #[test]
    fn a_held_rig_lets_the_dwell_converge_where_an_orbit_cannot() {
        let b = bounds((-50.0, -30.0, -1.0), (50.0, 30.0, 0.2));
        let cam = [1.0, 6.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let vp_of = |(c, yaw, pitch, dist, _roll, fov): (Vec3, f32, f32, f32, f32, f32)| {
            let eye = c + dist * Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), pitch.cos() * yaw.cos());
            Mat4::perspective_rh(fov.to_radians(), 16.0 / 9.0, 0.1, 5000.0) * Mat4::look_at_rh(eye, c, Vec3::Y)
        };
        let settled = GlyphPtState { live: true, generation: 3, settled: true };
        // Held: the same tuple every frame.
        let mut prev_vp = Mat4::ZERO;
        let mut spp = 0u32;
        for _ in 0..4 {
            let vp = vp_of(glyph_camera_rig(true, &cam, &b, (1920, 1080), 45.0).unwrap());
            let moved = vp != prev_vp;
            if pathtrace_restarts(moved, false, false, pathtrace_active(false, settled)) {
                spp = 0;
            }
            prev_vp = vp;
            spp += 1;
        }
        assert_eq!(spp, 4, "a held camera restarts once (the first frame) and then accumulates");
        // Orbiting: a yaw that advances every frame restarts every frame.
        let mut prev_vp = Mat4::ZERO;
        let mut spp = 0u32;
        for f in 0..4 {
            let yaw = 0.01 * f as f32;
            let vp = vp_of((b.center(), yaw, 0.1, 300.0, 0.0, 45.0));
            let moved = vp != prev_vp;
            if pathtrace_restarts(moved, false, false, pathtrace_active(false, settled)) {
                spp = 0;
            }
            prev_vp = vp;
            spp += 1;
        }
        assert_eq!(spp, 1, "an orbiting camera never accumulates past one sample");
    }
}

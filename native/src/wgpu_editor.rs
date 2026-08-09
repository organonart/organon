//! #593 Tier 2 — **the custom wgpu `Editor`**: the real renderer and the real interface, in
//! the window nih-plug hands us, on one device.
//!
//! # What this is
//!
//! Tier 0 ([`crate::editor_probe`]) proved on hardware that `Editor::spawn`'s
//! [`ParentWindowHandle`] can carry a `wgpu::Surface`. Tier 1 hoisted the editor body into
//! [`crate::editor_ui`] so a second host can draw the identical interface. This file is the
//! payload those two exist for — it **grows the probe's `on_frame`** from a cycling clear into:
//!
//! 1. [`World::render_into`] — the scene itself, at the window's full resolution (#582's seam),
//! 2. the interface over it, via the vendored `egui-wgpu` (#569) calling [`crate::editor_ui`],
//! 3. input from **baseview**, translated by [`crate::baseview_input`] (#599),
//!
//! while nih-plug's wrapper keeps owning the params, so [`ParamSetter`] stays real. That last
//! point is why route C was chosen over forking nih-plug or abstracting the param path; it is
//! settled and not relitigated here.
//!
//! The handle-adaptation chain (nih-plug's enum → rwh 0.5 → rwh 0.6) is **not re-derived** —
//! it is [`crate::editor_probe`]'s, reused verbatim, because it is the part that has actually
//! run on a Mac.
//!
//! # What it deliberately does not do
//!
//! - **It draws no world-side interface.** [`FrameTarget::ui_scale_factor`] is `None`, which the
//!   world reads as "draw no interface" — the UI is *this* file's egui pass instead. That is what
//!   let Tier 2 land without touching the world's last winit coupling; Tier 3 then removed
//!   `ui_window` outright.
//! - **No `Shared` / IPC / `LAYOUT_VERSION` change.** The UI shares the renderer's process, so
//!   nothing new crosses IPC. If a later step wants one, the design drifted. (Held through all
//!   five tiers.)
//!
//! # What Tier 4 added, and why nothing in this file changed
//!
//! Tier 2 drew the world into this surface correctly on the very first run, **and it was
//! invisible** — `editor_ui`'s `CentralPanel` is opaque, and the #554 mirror pane painted a
//! 640×360 photograph over exactly the region the scene occupied. "Nothing rendered" and
//! "rendered, then covered" look identical in a screenshot.
//!
//! Tier 4 is the two-line answer, and both lines are in `lib.rs`, not here: the central region's
//! frame becomes transparent when the host has drawn the world behind it
//! (`EditorCtx::scene_behind` → `theme::workspace_frame`), and the mirror pane is
//! `#[cfg(not(mind-edition))]` along with `frame_ring`, `Mirror` and everything that paced or
//! sized them. This file already did its job; what changed is that the interface stopped
//! covering it up.
//!
//! # What #621 added — the camera
//!
//! Through Tier 4 and #617 this file's [`WindowHandler::on_event`] handed every event to egui and
//! **never touched [`World`]**, so both viewport modes were look-only: a 2560×1720 scene with no
//! orbit and no zoom. #621 closes it with one line in [`on_frame`](WindowHandler::on_frame) —
//! drain `PresetUi::scene_input`'s gesture into `World::apply_camera_input` — and the whole
//! decision behind that line is in [`crate::scene_input`], including the measurement of why the
//! obvious route (`mind_shell::PointerRouter`, at `on_event`) cannot work in a host that draws a
//! `CentralPanel`. `on_event` is unchanged.
//!
//! # How to run it
//!
//! Gated on `mind-edition` (so the shipping plugin cdylib cannot move), and **on by default**
//! within it since #593 closed:
//!
//! ```sh
//! ./organon-mind --backend dummy
//! ```
//!
//! `--backend dummy` is required: real CoreAudio hard-aborts the standalone (#579).
//!
//! **`ORGANON_EDITOR_WGPU=0` is the way back**, and it is a bring-up fallback rather than a
//! supported mode: it opens the `nih_plug_egui` editor, which in a mind-edition build has **no
//! viewport at all** (the #554 mirror pane left Mind's path at Tier 4). You keep the docks,
//! the telemetry and model loading; you lose the specimen. Use it if this editor fails to come
//! up on some machine, and file what happened.
//!
//! ⚠️ **This gate was `=1`-to-opt-in for all five tiers, and inverting it was the last item.**
//! House invariant #6 — new capability defaults to inert — is what held it off, and its own
//! documented exit condition was the Mac pass. That happened 2026-08-03 (one window, 2560×1720,
//! sustained 60.0 fps, both #617 modes exercised), so the invariant is *completing* here, not
//! being broken. Until it flipped, plain `organon-mind` shipped with no viewport whatsoever —
//! strictly less than before Tier 4, which is the state this change ends.
//!
//! # ⚠️ What is measured and what is asserted
//!
//! This file was **written** in a container with no macOS SDK and no way to run a plugin host,
//! and said so — "nothing here has rendered a pixel" — for as long as that was true. It is not
//! any more, and the record belongs here rather than only in the doc:
//!
//! - **Mac, 2026-08-03** — one window, `2560×1720` Retina, sustained **60.0 fps**,
//!   `$TMPDIR/organon-mind-frame.bin` never created with a visual running alongside, both #617
//!   viewport modes exercised.
//! - **Linux/X11, RTX 5060 Ti** — the X11 arm of the same path: surface up, the world's own
//!   pipelines created lazily inside `render_into`, `editor_ui` drawing the real interface,
//!   `66.7 fps`, zero wgpu validation lines.
//!
//! **Still asserted, not measured**, and worth knowing before trusting this file:
//!
//! - **`surface.configure` on a live parented `NSView` under a scale change** — dragging the
//!   window between displays of different scale. `MIND_ARCHITECTURE.md` §2.4's last
//!   never-executed item; `SurfaceAction` has no re-create variant, which is what makes it a
//!   reconfigure rather than a lifetime question, but the reconfigure itself has not run.
//! - **The keymap.** `on_event` below still feeds egui and nothing else, so the visual's
//!   shortcuts (**F**, **H**, **U**, **P**, the recorder keys) do not reach this window — the
//!   deliberately-open half of **#621**, and a projector concern in a docked pane.
//!
//! ⚠️ **The camera half of that bullet is closed** (#621): the scene above is one you can
//! navigate, not only watch. `editor_ui` publishes the gesture and `on_frame` drains it into
//! `World::apply_camera_input`, so orbit and zoom work in both #617 modes. Driven locally
//! 2026-08-04 and confirmed working; the itemised gate in the PR — the two modes separately,
//! a card drag not orbiting, tracking after the workstation has scrolled — is the finer pass.
//!
//! A cloud session can still only establish the tests at the bottom (the surface-lifetime
//! policy, the frame-order invariant, the gate) plus both editions compiling. Anything changed
//! here is **"green and ready to deploy"**, never "verified working", until it reaches hardware
//! again.

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use baseview::{
    Event, EventStatus, Size, Window, WindowHandle, WindowHandler, WindowInfo, WindowOpenOptions,
    WindowScalePolicy,
};
use nih_plug::prelude::{Editor, GuiContext, ParamSetter, ParentWindowHandle};

use crate::editor_probe::{display_handle_06, window_handle_06, ParentWindowHandleAdapter};
use crate::egui_platform::WindowGeometry;
use crate::baseview_platform;
use crate::world::ui_layer::UiLayer;
use crate::world::{FrameTarget, World};
use crate::{baseview_input, preset, EditorCtx};

use raw_window_handle_05::{HasRawDisplayHandle, HasRawWindowHandle};

/// The environment variable that *disables* this editor. Checked in `lib.rs`'s `editor()`.
///
/// Note the sense: this used to be the opt-*in* (`=1`) while the editor was being built tier by
/// tier. Since #593 closed it is the opt-*out*, and the name outlived the polarity — renaming it
/// would break the one escape hatch for anyone who has it written down.
pub const WGPU_EDITOR_ENV: &str = "ORGANON_EDITOR_WGPU";

/// Whether the wgpu editor is enabled, given the raw value of [`WGPU_EDITOR_ENV`].
///
/// Split from the `std::env` read so the gate is testable without mutating process environment
/// from a parallel test runner — same shape as the probe's.
///
/// **Only an exact `"0"` disables**, mirroring the strictness of the `=1` gate this replaced. A
/// typo (`=fasle`, `=of`) therefore leaves you in the *default* state rather than silently
/// dropping you into the viewport-less editor — the failure that would be hardest to recognise,
/// because in Organon Mind an empty viewport and a broken app look identical. That confusion has
/// already cost this project real time twice (`STATUS.md`'s "no model loaded looks exactly like
/// broken"), which is also why taking the fallback logs a warning rather than going quiet.
pub fn wgpu_editor_enabled_from(value: Option<&str>) -> bool {
    value != Some("0")
}

/// Whether the wgpu editor is enabled in *this* process.
pub fn wgpu_editor_enabled() -> bool {
    wgpu_editor_enabled_from(std::env::var(WGPU_EDITOR_ENV).ok().as_deref())
}

/// How often `on_frame` reports its counter, in frames.
///
/// **This exists for the same reason the probe's cycling colour does**, and it earned its place
/// the first time this editor ran: the interface it draws is *static when idle*, so two captures
/// seconds apart are byte-identical whether the render loop is alive or wedged. A screenshot
/// cannot tell those apart; a climbing counter and a frame interval can. #582 shipped a dead
/// redraw loop that looked exactly like a working one.
pub const FRAME_LOG_EVERY: u64 = 120;

// ─────────────────────────────────────────────────────────────────────────────
// The surface-lifetime policy — MIND_ARCHITECTURE.md §2.4's open question
// ─────────────────────────────────────────────────────────────────────────────

/// What a size change should do to the swapchain.
///
/// **This enum is Tier 2's answer to §2.4's open question**, made explicit so it can be
/// reasoned about and tested rather than left implicit in a control flow nobody can run here.
///
/// The question inherited from Tier 0 is whether a `Surface` can outlive the `NSView` it was
/// created from across an in-process re-create. The answer this file gives is: **it never
/// gets the chance.** A surface is created exactly once, inside
/// [`EditorWindowHandler::new`], from the baseview window that owns the handler — and a size
/// or scale change only ever *reconfigures* it. There is no code path that drops and rebuilds
/// a `Surface` while its view lives, because [`SurfaceAction::Reconfigure`] is the only
/// response to a resize.
///
/// That converts the risk from "does the rebuild order work" into "there is no rebuild",
/// which is a much smaller claim and one the type system helps hold. ⚠️ It is still a claim
/// about *this* code, not a measurement: what remains unproven on hardware is that
/// `surface.configure` on a live parented `NSView` behaves under a scale-factor or display
/// change. That needs the Mac, and it is the one thing the deploy must exercise deliberately —
/// drag the window between displays of different scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceAction {
    /// The size is unusable (a minimised or zero-area window). Skip the frame; touch nothing.
    Skip,
    /// Nothing changed — the swapchain is already correct.
    None,
    /// Reconfigure the existing surface in place. **Never re-create it.**
    Reconfigure { width: u32, height: u32 },
}

/// Decide what a reported size means for the swapchain.
///
/// Pure so the policy above is a test rather than a comment. A zero dimension is `Skip` and not
/// `Reconfigure { 1, 1 }`: configuring a 1×1 swapchain for a minimised window churns the
/// surface for a frame nobody sees, and the next real size reconfigures it again anyway.
pub fn surface_action(current: (u32, u32), reported: (u32, u32)) -> SurfaceAction {
    if reported.0 == 0 || reported.1 == 0 {
        return SurfaceAction::Skip;
    }
    if reported == current {
        return SurfaceAction::None;
    }
    SurfaceAction::Reconfigure { width: reported.0, height: reported.1 }
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU bring-up
// ─────────────────────────────────────────────────────────────────────────────

/// Build a [`UiLayer`] on the baseview arm — the mirror of `winit_platform::ui_layer`.
///
/// It lives here rather than in `baseview_platform` because it names `world::ui_layer`, which is
/// gated on `mind-edition`; the trait impl itself has to be ungated and in the library (the
/// orphan rule — see that module's docs).
///
/// `geometry` seeds the first frame: baseview volunteers a `WindowInfo` only on `Resized` and
/// never at open, so without it egui lays its first frame out against an empty screen rect.
fn build_ui_layer(
    device: &wgpu::Device,
    geometry: WindowGeometry,
    format: wgpu::TextureFormat,
) -> UiLayer<baseview_input::State> {
    let ctx = egui::Context::default();
    let max_texture_side = device.limits().max_texture_dimension_2d as usize;
    let platform = baseview_input::State::new(
        ctx.clone(),
        egui::ViewportId::ROOT,
        &baseview_platform::window_info(geometry),
        // `None` = follow the window's own scale factor — the winit arm's choice too.
        None,
        None,
        Some(max_texture_side),
    );
    UiLayer::new(device, ctx, platform, format)
}

/// baseview's `WindowInfo` → the [`WindowGeometry`] the #593 Tier 3 seam speaks.
///
/// The editor tracks `WindowInfo` because that is what baseview volunteers on `Resized`; the UI
/// layer wants the geometry pair. One place to convert, so a stale scale cannot creep in.
fn geometry_of(info: &WindowInfo) -> WindowGeometry {
    let size = info.physical_size();
    WindowGeometry::new((size.width, size.height), info.scale() as f32)
}

/// The swapchain this editor owns. The `Device`/`Queue` do **not** live here — they are moved
/// into the [`World`] by `attach_gpu`, which is the arrangement every other host uses, and are
/// borrowed back through `World::device()` / `World::queue()` for the egui pass.
struct EditorSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// Build instance → surface → adapter → device from a live baseview window, and attach the
/// world to it.
///
/// The feature/limit negotiation is **`bin/visual.rs`'s, deliberately duplicated rather than
/// simplified**: the cube pipeline needs `max_bind_groups` raised past wgpu's default of 4
/// (five groups since #152 T3 — without it the shader module fails to create at startup), and
/// the RT / timestamp / adapter-specific-format features are what the renderer probes for.
/// A device built with the probe's `Limits::default()` would bring the window up and then fail
/// to create pipelines, which is exactly the "compiles, draws black" failure this thread keeps
/// paying for.
fn bring_up(window: &Window, world: &mut World, size: (u32, u32)) -> Result<EditorSurface, String> {
    let instance = wgpu::Instance::default();

    // Both handles come from the **baseview** window, never from `ParentWindowHandle` — the
    // latter's `X11Window(u32)` carries no display connection. See `editor_probe`'s docs.
    let raw_window = window_handle_06(window.raw_window_handle())
        .ok_or_else(|| String::from("baseview window handle is not usable by wgpu"))?;
    let raw_display = display_handle_06(window.raw_display_handle())
        .ok_or_else(|| String::from("baseview display handle is not usable by wgpu"))?;

    // SAFETY: both handles come from the baseview window that owns the handler this surface is
    // stored inside, so the surface is dropped as part of tearing that window down. See
    // `SurfaceAction` on why it is never re-created while the view lives.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display),
            raw_window_handle: raw_window,
        })
    }
    .map_err(|e| format!("create_surface_unsafe: {e}"))?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("request_adapter: {e}"))?;

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
    // wgpu gates EXPERIMENTAL_* behind an acknowledgement token on top of the feature bit.
    // SAFETY: wgpu's "there may be UB bugs in experimental APIs" waiver; all ray-query use is
    // contained in rt.rs (#195's churn rule), exactly as in `bin/visual.rs`.
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
        label: Some("organon-mind-editor"),
        required_features,
        required_limits,
        experimental_features,
        ..Default::default()
    }))
    .map_err(|e| format!("request_device: {e}"))?;

    let caps = surface.get_capabilities(&adapter);
    // SDR only, on purpose: the HDR/EDR swap is a `CAMetalLayer` negotiation the *window owner*
    // performs, and in a plugin-parented view that layer is not ours to re-tag. `hdr_max` stays
    // 1.0 below to match. Full Organon's separate visual window keeps the HDR path.
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.0.max(1),
        height: size.1.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
        color_space: Default::default(),
    };
    surface.configure(&device, &config);

    eprintln!(
        "[mind editor] surface up: {}×{} {:?} on {}",
        config.width,
        config.height,
        format,
        adapter.get_info().name
    );

    // Everything downstream of the `Device` is the world's, and is identical for every host.
    // `ui_window: None` — this editor draws the interface itself (see the module docs).
    // `ui: None` — #593 Tier 3 made the UI layer a host-built argument. This editor draws its
    // own (see the module docs), so the world is handed none and `ui_scale_factor` stays `None`
    // on every frame: two halves of the same statement, that the world draws no interface here.
    world.attach_gpu(device, queue, format, None, coopmat_available, f16_available);
    Ok(EditorSurface { surface, config })
}

// ─────────────────────────────────────────────────────────────────────────────
// The window handler — the frame
// ─────────────────────────────────────────────────────────────────────────────

/// Everything downstream of a successful bring-up.
struct Gpu {
    surface: EditorSurface,
    /// #593 Tier 3 — the shared UI layer on the **baseview arm**, rather than a second
    /// hand-rolled egui pass. `UiLayer<State>` is the same type the visual host uses; only the
    /// `EguiPlatform` differs, which is the whole point of the seam.
    ui: UiLayer<baseview_input::State>,
}

/// Apply what `EguiPlatform::handle_platform_output` handed back as a plan.
///
/// #593 Tier 3 made the deferred half **data** rather than a call, because a baseview window can
/// only be acted on from inside a `WindowHandler` callback — which is exactly where this runs.
/// The winit arm's `Deferred` is `()`; this is the arm that has real work to do.
fn apply_platform_actions(window: &mut Window, actions: baseview_input::PlatformActions) {
    if let Some(text) = &actions.copy_text {
        baseview::copy_to_clipboard(text);
    }
    // `open_url` is deliberately reported and not acted on by `baseview_input` — opening a
    // browser from a plugin editor is the host's call, not the translation's. Nothing in the
    // editor emits one today; when something does, it gets a decision rather than a surprise.
    let _ = &actions.open_url;
    // macOS: baseview's cursor setter panics on some icons, so #599 reports the change and lets
    // the host decide. Off macOS it is a plain set, deduped upstream.
    #[cfg(not(target_os = "macos"))]
    if let Some(cursor) = actions.cursor {
        window.set_mouse_cursor(cursor);
    }
    #[cfg(target_os = "macos")]
    let _ = (window, actions.cursor);
}

/// The editor's `baseview::WindowHandler`: one world, one surface, one egui layer.
struct EditorWindowHandler {
    world: World,
    gpu: Option<Gpu>,
    cx: Arc<EditorCtx>,
    /// nih-plug's context, kept for the whole life of the window so a real [`ParamSetter`] can
    /// be built each frame. **This is the object route C exists to preserve**: the wrapper still
    /// owns the params, so `setter.begin_set_parameter` reaches the host's undo/automation
    /// machinery exactly as it does under `nih_plug_egui`.
    gui_ctx: Arc<dyn GuiContext>,
    /// The editor's own UI state (preset rail, keymap editor), the same type and the same
    /// lazy-load the `create_egui_editor` build closure performs.
    state: preset::PresetUi,
    /// The latest window geometry. baseview volunteers this only on `Resized`, and never at open
    /// time, so it is seeded from the requested size and kept current here.
    info: WindowInfo,
    frames: u64,
    /// When the last [`FRAME_LOG_EVERY`] report went out, so the log carries a real frame
    /// interval rather than just a count. See that constant on why this is not optional.
    last_report: std::time::Instant,
    /// #617 Tier 1 — the workstation viewport's own render target. `None` in immersive mode,
    /// where the world is drawn straight into the swapchain image and there is nothing to sample.
    scene: Option<ScenePane>,
}

/// The offscreen target the world is drawn into for the **workstation** viewport (#617 Tier 1).
///
/// Recreated only when the pane's pixel size changes — a texture per frame would be a new bind
/// group per frame, which is the classic way to make a viewport leak.
struct ScenePane {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// Physical pixels, so a Retina pane renders at Retina resolution rather than being upscaled.
    size: (u32, u32),
}

/// The format the world **renders** the workstation viewport in — sRGB, so the hardware encodes
/// the composite's linear output on the way into the texture.
///
/// ⚠️ **This pane needs two formats, and getting it wrong is a silent brightness bug.** egui's
/// shader calls its texture sample `tex_gamma` and then runs `linear_from_gamma_rgb` on it: it
/// assumes **every** texture it samples is already sRGB-encoded. So the bytes in this texture must
/// be gamma-encoded, and they must reach egui *without* being decoded again.
///
/// One texture, two views, is what satisfies both:
///
/// - the world renders through an **`…Srgb` view** ([`SCENE_PANE_FORMAT`]) — its composite writes
///   linear and the hardware encodes once, exactly as it does into the sRGB swapchain;
/// - egui samples through a **plain `Rgba8Unorm` view** ([`SCENE_PANE_SAMPLE_FORMAT`]), which
///   reads those gamma bytes raw. Sampling the `…Srgb` view instead would have the hardware
///   decode them back to linear and egui would linearize a second time.
///
/// The first cut used a single `Rgba8Unorm` texture, following `register_native_texture`'s
/// documented "must be `Rgba8Unorm`" — which is about the *view* egui binds, not about where the
/// encode happens. The world then stored linear bytes, egui linearized them again, and the pane
/// came out far too dark: measured against the same scene in immersive mode, sky that read
/// `0.431 0.436 0.336` came back `0.238 0.219 0.120`. That A/B is the only thing that catches
/// this — it renders, it animates, it is simply wrong, and nothing errors.
const SCENE_PANE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// The view format egui samples the pane through. See [`SCENE_PANE_FORMAT`] for why it differs.
const SCENE_PANE_SAMPLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Clear the swapchain image so the egui pass has something defined to load over (#617 Tier 1).
///
/// Only workstation mode needs this; immersive mode's world write covers every pixel. The colour
/// is irrelevant in practice — the workstation's panels paint over essentially all of it — so it
/// is plain black rather than a theme token, which would tie this file to the palette for pixels
/// nobody sees.
fn clear_frame(texture: &wgpu::Texture, world: &World) {
    let (Some(device), Some(queue)) = (world.device(), world.queue()) else { return };
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pane-clear") });
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("pane-clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    queue.submit(Some(encoder.finish()));
}

/// The pane size used on the one frame before `editor_ui` has reserved a rect. Fractions of the
/// window rather than fixed points, so the first frame is roughly right at any window size — it
/// is replaced by the real rect on the very next frame and never seen again.
const DEFAULT_PANE_W_FRAC: f32 = 0.6;
const DEFAULT_PANE_H_FRAC: f32 = 0.3;

impl EditorWindowHandler {
    fn new(
        window: &mut Window,
        cx: Arc<EditorCtx>,
        gui_ctx: Arc<dyn GuiContext>,
        info: WindowInfo,
    ) -> Self {
        let mut world = World::new();
        let size = (info.physical_size().width, info.physical_size().height);
        let gpu = match bring_up(window, &mut world, size) {
            Ok(surface) => {
                // Borrowed back out of the world — `attach_gpu` took ownership, which is the
                // arrangement every host uses.
                let device = world.device().expect("device present after attach_gpu");
                let format = surface.config.format;
                // The baseview arm of #593 Tier 3's seam. Same `UiLayer` the visual host builds;
                // only the `EguiPlatform` differs.
                let ui = build_ui_layer(device, geometry_of(&info), format);
                Some(Gpu { surface, ui })
            }
            Err(e) => {
                // Loud, not fatal — a window that opens and says why it is blank tells you far
                // more than an editor the host simply loses (the probe's rule).
                eprintln!("[mind editor] GPU bring-up FAILED: {e}");
                None
            }
        };
        Self {
            world,
            gpu,
            cx,
            gui_ctx,
            state: preset::PresetUi::default(),
            info,
            frames: 0,
            last_report: std::time::Instant::now(),
            scene: None,
        }
    }

    /// Draw the world into the workstation viewport's own texture, and hand that texture to egui
    /// (#617 Tier 1).
    ///
    /// Sized from the rect `editor_ui` reserved on the **previous** frame — the scene has to be
    /// drawn before the interface that reserves its rect runs, so this is one frame behind by
    /// construction. `PresetUi::scene_pane_rect` carries why that is inherent and harmless.
    fn render_scene_pane(&mut self, window_px: (u32, u32)) {
        let scale = geometry_of(&self.info).scale();
        // No rect yet (frame one, or the very first frame after a mode switch): pick something
        // plausible so the pane opens with a scene in it rather than a hole, and let the UI
        // correct the size on the next frame.
        let (w_pt, h_pt) = match self.state.scene_pane_rect {
            Some(r) => (r.width(), r.height()),
            None => (
                window_px.0 as f32 / scale * DEFAULT_PANE_W_FRAC,
                window_px.1 as f32 / scale * DEFAULT_PANE_H_FRAC,
            ),
        };
        // Clamped, not asserted: egui can hand back a zero or negative rect for one frame while
        // a dock is being dragged, and a zero-sized texture is a validation error, not a blank
        // pane. The upper bound is a sanity rail against a runaway layout. #621 hoisted the
        // arithmetic into `scene_input::pane_pixels` so those two clamps are a test rather than
        // this comment.
        let (w, h) = crate::scene_input::pane_pixels((w_pt, h_pt), scale);

        let Some(device) = self.world.device() else { return };
        if self.scene.as_ref().is_none_or(|s| s.size != (w, h)) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mind-workstation-viewport"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_PANE_FORMAT,
                // RENDER_ATTACHMENT for the world's composite, TEXTURE_BINDING for egui's sampler.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                // The second view format egui samples through — see `SCENE_PANE_FORMAT`.
                view_formats: &[SCENE_PANE_SAMPLE_FORMAT],
            });
            // **Not the default view.** The default would inherit the texture's `…Srgb` format
            // and the hardware would decode on sample, undoing the encode egui is counting on.
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("mind-workstation-viewport-sample"),
                format: Some(SCENE_PANE_SAMPLE_FORMAT),
                ..Default::default()
            });
            self.scene = Some(ScenePane { texture, view, size: (w, h) });
            // The old id names a view that no longer exists — force a re-register below.
            self.state.scene_texture = None;
        }

        // Disjoint fields: `self.world` is borrowed mutably, `self.scene` immutably.
        let Some(pane) = self.scene.as_ref() else { return };
        self.world.render_to_texture(&pane.texture, pane.size, SCENE_PANE_FORMAT);

        // Register once per texture, not once per frame: re-registering every frame allocates a
        // bind group every frame, which is how an embedded viewport quietly leaks.
        if self.state.scene_texture.is_none() {
            let (Some(gpu), Some(device)) = (self.gpu.as_mut(), self.world.device()) else {
                return;
            };
            let scene_view = &self.scene.as_ref().expect("just rendered into it").view;
            let id = gpu.ui.register_scene_texture(device, scene_view, None);
            self.state.scene_texture = Some(id);
        }
    }

    /// The lazy load `create_egui_editor`'s build closure performs, so the two hosts open with
    /// identical state rather than one of them starting with an empty preset rail.
    fn ensure_state_loaded(&mut self) {
        if !self.state.loaded {
            self.state.presets = preset::load();
            for tab in preset::EditorTab::ALL {
                self.state.tab_presets[tab.index()] = preset::load_tab(tab);
            }
            self.state.loaded = true;
        }
        if !self.state.keymap_loaded {
            self.state.mapping = crate::keymap::KeyMapping::load();
            self.state.keymap_octave = 3; // open on the octave around middle C
            self.state.keymap_loaded = true;
        }
    }
}

impl WindowHandler for EditorWindowHandler {
    fn on_frame(&mut self, window: &mut Window) {
        self.frames += 1;
        self.ensure_state_loaded();
        let Some(gpu) = self.gpu.as_mut() else { return };

        let Some(device) = self.world.device() else { return };
        let frame = match gpu.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                f
            }
            // A hidden or covered window. Reconfiguring cannot un-occlude it — skip quietly.
            wgpu::CurrentSurfaceTexture::Occluded => return,
            _ => {
                gpu.surface.surface.configure(device, &gpu.surface.config);
                return;
            }
        };

        // ── 1. the scene ────────────────────────────────────────────────────
        // `ui_window: None` — the world draws no interface; the egui pass below is ours.
        // `hdr_max: 1.0` / `wide_gamut: false` — SDR only in a parented view (see `bring_up`).
        //
        // #617 Tier 1 — **two modes, and this is where they fork.**
        //
        // *Immersive* renders the world straight into the swapchain image, full size, and the
        // interface is drawn transparently over it below. That is #593 Tier 4's behaviour,
        // unchanged, and it is the right shape for an instrument you are watching.
        //
        // *Workstation* (the default) renders the world into a texture the size of the pane
        // `editor_ui` reserved, and egui paints that texture as an image inside the pane. The
        // scene becomes a widget: it clips, it scrolls with the panel, and it stops sitting
        // behind the interface's text. That is the shape a tool with docks and rails wants, and
        // it is the conventional one (Dear ImGui's `ImGui::Image`, egui's user textures).
        let size = (gpu.surface.config.width, gpu.surface.config.height);
        let format = gpu.surface.config.format;
        if self.state.immersive {
            self.scene = None; // drop the offscreen target; immersive never samples it
            self.state.scene_texture = None;
            let _requests = self.world.render_into(FrameTarget {
                texture: &frame.texture,
                size,
                format,
                presented: true,
                hdr_max: 1.0,
                wide_gamut: false,
                ui_scale_factor: None,
            });
        } else {
            self.render_scene_pane(size);
            // **The swapchain still has to be written.** In immersive mode the world fills it;
            // here nothing has, and `UiLayer::paint` loads rather than clears (it is a late pass
            // over a scene that was already there). Loading an image nothing wrote is undefined
            // content, and the workstation's panels do not provably cover every pixel — a
            // rounded dock corner is enough to leave one showing. One clear costs nothing.
            clear_frame(&frame.texture, &self.world);
        }
        // `FrameRequests` is deliberately dropped. Both fields are window-shaped and neither
        // applies to a view the host owns: `inner_size` is the #135 lock-window-to-output
        // request (a plugin editor cannot resize itself behind the host's back — that is what
        // `EguiState::set_requested_size` is for), and `title` addresses a titlebar a parented
        // NSView does not have. The world re-fires both while they differ, so ignoring them is
        // stable rather than sticky — which is exactly why they are *requests*.

        // ── 2. the interface, over it ───────────────────────────────────────
        let Some(gpu) = self.gpu.as_mut() else { return };
        let (Some(device), Some(queue)) = (self.world.device(), self.world.queue()) else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // The **same** function the `nih_plug_egui` editor calls (#593 Tier 1), given the same
        // `EditorCtx` and a real `ParamSetter` — so the two hosts cannot draw different
        // interfaces or move parameters differently.
        let setter = ParamSetter::new(self.gui_ctx.as_ref());
        let cx = self.cx.clone();
        let state = &mut self.state;
        let geometry = geometry_of(&self.info);
        // `UiLayer::paint` owns the run → tessellate → upload → `LoadOp::Load` pass, so the
        // scene it draws over is exactly what `render_into` just wrote.
        let deferred = gpu.ui.paint(device, queue, &view, geometry, |ctx| {
            crate::editor_ui(&cx, ctx, &setter, state);
        });

        // What `handle_platform_output` could not do itself: baseview only lends its window
        // inside a callback, and this is that callback. Dropping this is how the cursor stops
        // updating and ⌘C silently stops working.
        if let Some(actions) = deferred {
            apply_platform_actions(window, actions);
        }

        // ── 2b. the camera ──────────────────────────────────────────────────
        //
        // **#621 — the last hop, and the whole issue.** Tiers 2–4 and #617 built a
        // native-resolution viewport nobody could move the camera in, because `on_event` below
        // hands everything to egui and never reaches the world. `editor_ui` has just run, so
        // `scene_input` holds whatever this frame's drag and wheel asked for, in the units
        // `World::apply_camera_input` has always consumed.
        //
        // **Drained, not read** — a gesture applied twice is an orbit at double rate, and this
        // is the only reader. Applying it *after* the UI and *before* the next `render_into`
        // means a gesture lands in the very next frame; the alternative (route it at the
        // platform event) cannot work here at all — `scene_input`'s module docs carry the
        // measurement.
        for input in self.state.scene_input.gesture.take().inputs() {
            self.world.apply_camera_input(input);
        }

        // ── 3. present ──────────────────────────────────────────────────────
        self.world.present(frame);

        // Liveness, out loud. See `FRAME_LOG_EVERY`: the interface is static when idle, so this
        // is the only thing that distinguishes "rendering" from "wedged" without a debugger.
        if self.frames % FRAME_LOG_EVERY == 0 {
            let dt = self.last_report.elapsed();
            self.last_report = std::time::Instant::now();
            eprintln!(
                "[mind editor] frame {} — {:.1} fps over the last {} frames ({}×{})",
                self.frames,
                FRAME_LOG_EVERY as f64 / dt.as_secs_f64().max(f64::EPSILON),
                FRAME_LOG_EVERY,
                size.0,
                size.1,
            );
        }
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        // Track geometry first: egui lays out against `self.info`, and baseview volunteers a
        // `WindowInfo` only here.
        if let Event::Window(baseview::WindowEvent::Resized(info)) = &event {
            self.info = *info;
            let reported = (info.physical_size().width, info.physical_size().height);
            if let Some(gpu) = self.gpu.as_mut() {
                let current = (gpu.surface.config.width, gpu.surface.config.height);
                if let SurfaceAction::Reconfigure { width, height } =
                    surface_action(current, reported)
                {
                    gpu.surface.config.width = width;
                    gpu.surface.config.height = height;
                    if let Some(device) = self.world.device() {
                        // Reconfigure in place. **Never re-create** — see `SurfaceAction`.
                        gpu.surface.surface.configure(device, &gpu.surface.config);
                    }
                }
            }
        }

        let geometry = geometry_of(&self.info);
        let Some(gpu) = self.gpu.as_mut() else {
            return EventStatus::Ignored;
        };
        // #593 Tier 3: `UiEvent` carries both halves — `target` (who owns the gesture) and
        // `response` (what the platform must be told). **This is the reader `response` was
        // waiting for**, which is why its `#[allow(dead_code)]` is gone.
        let ui_event = gpu.ui.on_window_event(geometry, &event);
        match ui_event.response {
            // `status()` is not advisory. `Ignored` hands the event back to the host — which is
            // how a DAW keeps playing on the space bar while a plugin window has focus — and
            // `AcceptDrop` is the gate the entire drag gesture passes through on macOS and
            // Windows: without it a dropped `.gguf` bounces back. Reporting `Captured` for
            // everything is how a plugin steals the transport.
            Some(response) => response.status(),
            // The UI layer is hidden (it never consulted the platform), so the event is not
            // ours. A hidden UI must not silently eat a drag.
            None => EventStatus::Ignored,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The Editor
// ─────────────────────────────────────────────────────────────────────────────

/// Organon Mind's own `nih_plug::editor::Editor` — the scene and the interface on one device.
///
/// Holds no GPU state: everything is built inside `spawn` and dropped with the window, so a
/// re-create is a fresh bring-up rather than a resurrection of stale handles.
pub struct WgpuEditor {
    cx: Arc<EditorCtx>,
    /// The editor's persisted size, shared with `EguiState` so the #520 T2 resize survives.
    editor_state: Arc<nih_plug_egui::EguiState>,
    scale_bits: AtomicU32,
    open: Arc<AtomicBool>,
}

impl WgpuEditor {
    pub(crate) fn new(cx: EditorCtx, editor_state: Arc<nih_plug_egui::EguiState>) -> Self {
        Self {
            cx: Arc::new(cx),
            editor_state,
            scale_bits: AtomicU32::new(0),
            open: Arc::new(AtomicBool::new(false)),
        }
    }

    fn scaling_factor(&self) -> Option<f32> {
        match self.scale_bits.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f32::from_bits(bits)),
        }
    }
}

impl Editor for WgpuEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let (w, h) = self.editor_state.size();
        let scale = self.scaling_factor();
        // baseview hands `WindowInfo` back only on `Resized`, so the first frame's layout comes
        // from this seed. `1.0` where the host named no factor: macOS never sets one and the
        // real backing scale arrives with the first `Resized`.
        let info = WindowInfo::from_logical_size(
            Size::new(w as f64, h as f64),
            scale.unwrap_or(1.0) as f64,
        );
        let cx = self.cx.clone();
        let window = Window::open_parented(
            &ParentWindowHandleAdapter(parent),
            WindowOpenOptions {
                title: String::from("Organon Mind"),
                size: Size::new(w as f64, h as f64),
                // Same policy as `vendor/nih_plug_egui`: honour the host's factor when it gave
                // one, otherwise let the platform decide.
                scale: scale
                    .map(|f| WindowScalePolicy::ScaleFactor(f as f64))
                    .unwrap_or(WindowScalePolicy::SystemScaleFactor),
                // `None` = no GL context: this editor draws with wgpu, which is the point.
                gl_config: None,
            },
            move |window| EditorWindowHandler::new(window, cx, context, info),
        );
        self.open.store(true, Ordering::Release);
        eprintln!("[mind editor] #593 Tier 2 — wgpu editor spawned ({w}×{h})");
        Box::new(WgpuEditorHandle { open: self.open.clone(), window })
    }

    fn size(&self) -> (u32, u32) {
        self.editor_state.size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // While the window is up there is nowhere to put a new scale factor, so refuse it —
        // Ableton does try. Mirrors `nih_plug_egui` and the probe.
        if self.open.load(Ordering::Acquire) {
            return false;
        }
        self.scale_bits.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    // The interface repaints on baseview's frame timer, so a parameter change needs no
    // targeted invalidation — the next frame reads the new value. Same posture as
    // `nih_plug_egui`'s editor, which also repaints unconditionally.
    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}

/// The handle nih-plug's wrapper holds while the editor is open. Dropping it closes the window,
/// which is what drops the `Surface` before the view it was created from goes away.
struct WgpuEditorHandle {
    open: Arc<AtomicBool>,
    window: WindowHandle,
}

// SAFETY: identical to `nih_plug_egui`'s and the probe's. `WindowHandle` is `!Send` because it
// wraps raw platform pointers; nih-plug's wrapper only moves this box between threads and only
// touches the window from the GUI thread that spawned it.
unsafe impl Send for WgpuEditorHandle {}

impl Drop for WgpuEditorHandle {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Release);
        // Explicit, for the same reason `nih_plug_egui` does it: dropping the handle does not
        // reliably close the window on its own.
        self.window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── the env gate ────────────────────────────────────────────────────────

    #[test]
    fn wgpu_editor_gate_needs_exactly_zero_to_disable() {
        assert!(!wgpu_editor_enabled_from(Some("0")));
    }

    #[test]
    fn wgpu_editor_gate_ignores_everything_else() {
        for v in [Some(""), Some("1"), Some("false"), Some("no"), Some("2"), Some("fasle")] {
            assert!(wgpu_editor_enabled_from(v), "{v:?} should not disable the wgpu editor");
        }
    }

    /// **The #593 close-out, pinned.** For all five tiers this asserted the opposite — house
    /// invariant #6, new capability defaults to inert — and the gate's documented exit condition
    /// was the Mac pass, which happened 2026-08-03. With nothing set, Organon Mind must now open
    /// the editor that *has* a viewport.
    ///
    /// Do not "restore" this to the old polarity. Unset used to mean the `nih_plug_egui` editor,
    /// and since Tier 4 gated the #554 mirror out of Mind's path that editor has no viewport at
    /// all — so the old default shipped an instrument that could not show you the model.
    #[test]
    fn wgpu_editor_is_the_default() {
        assert!(wgpu_editor_enabled_from(None));
    }

    // ── the surface-lifetime policy (MIND_ARCHITECTURE.md §2.4) ─────────────

    /// The claim Tier 2 makes about §2.4's open question: a resize **reconfigures**, so no
    /// `Surface` is ever dropped and rebuilt while its `NSView` is alive. If this ever returns
    /// a "recreate" variant, that claim is gone and §2.4 has to be re-opened.
    #[test]
    fn a_resize_only_ever_reconfigures() {
        assert_eq!(
            surface_action((640, 360), (1280, 720)),
            SurfaceAction::Reconfigure { width: 1280, height: 720 }
        );
        // The enum has no re-create variant *by construction* — this is the test that would
        // fail if someone added one and wired it here.
        match surface_action((640, 360), (800, 600)) {
            SurfaceAction::Reconfigure { .. } => {}
            other => panic!("a resize must reconfigure in place, got {other:?}"),
        }
    }

    #[test]
    fn an_unchanged_size_does_nothing() {
        assert_eq!(surface_action((1280, 720), (1280, 720)), SurfaceAction::None);
    }

    /// A minimised window reports a zero dimension. Configuring a 1×1 swapchain for it churns
    /// the surface for a frame nobody sees; skipping leaves the last good config in place.
    #[test]
    fn a_zero_dimension_is_skipped_not_clamped() {
        for reported in [(0, 720), (1280, 0), (0, 0)] {
            assert_eq!(
                surface_action((1280, 720), reported),
                SurfaceAction::Skip,
                "{reported:?} should be skipped"
            );
        }
    }
}

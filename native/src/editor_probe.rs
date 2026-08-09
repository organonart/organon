//! #593 Tier 0 — **the route-C probe**: a `wgpu::Surface` on the parent view the host
//! hands `Editor::spawn`, and the skeleton Tier 2 grows into.
//!
//! # Why this file exists
//!
//! #593's plan ("route C": replace `nih_plug_egui`'s baseview+glow editor with our own
//! [`Editor`] that draws the real renderer inside the plugin window) rests on one claim
//! nobody had compiled: that `Editor::spawn`'s [`ParentWindowHandle`] can become a
//! `wgpu::Surface` with a workable lifetime. This repo's record on unchecked API claims is
//! the reason the tier exists — #554 recorded *in four files* that `egui-wgpu` could never
//! pair with wgpu 30 (false: eight mechanical fixes), and #572's Tier 1 was written up as
//! fact and settled false by a four-line probe.
//!
//! # The chain, and the two places it can go wrong
//!
//! 1. nih-plug's [`ParentWindowHandle`] is **nih-plug's own three-variant enum**, not a
//!    raw-window-handle type. [`ParentWindowHandleAdapter`] adapts it to **rwh 0.5**, the
//!    version baseview speaks — the same shape as `vendor/nih_plug_egui/src/editor.rs`.
//! 2. `baseview::Window::open_parented` opens a child window on it. baseview stays: it is
//!    what normalizes NSEvent/X11/Win32 into cross-platform events. Only the layer *above*
//!    it (the renderer) becomes ours.
//! 3. [`window_handle_06`] / [`display_handle_06`] convert **that baseview window's**
//!    window *and* display handles to **rwh 0.6** (`wgpu::rwh` — wgpu 30 already re-exports
//!    raw-window-handle 0.6, so no second manifest entry is needed for it).
//! 4. `Instance::create_surface_unsafe` builds a `Surface<'static>` from the pair.
//!
//! **Both handles must come from the baseview window, never from `ParentWindowHandle`.**
//! The latter's `X11Window(u32)` carries no display connection, and rwh 0.6's Xlib/Xcb
//! *display* handles need one. `Surface<'static>` is the answer to the lifetime question:
//! we own the drop order, and [`ProbeEditorHandle`]'s `Drop` closes the window, which drops
//! the surface as part of tearing that window down rather than after it.
//!
//! ⚠️ **That ordering is asserted, not measured.** It is what the code is written to do;
//! no run has ever exercised it. Bring-up and teardown are measured (Mac, 2026-08-01:
//! three bring-ups, three clean exits through this `Drop`, exit 0, no wgpu validation
//! output). What has never executed is a **surface outliving its view across a re-create
//! inside one process** — see the open question in `MIND_ARCHITECTURE.md` §2.4. Organon
//! Mind is standalone-only and always will be, so no host ever closes and reopens this
//! editor; the risk survives only on in-process paths (a resize or scale-factor change
//! that rebuilds the surface, or a Tier 2 editor that rebuilds its window), and Tier 2 is
//! where it gets answered.
//!
//! # The colour cycles, and that is load-bearing
//!
//! [`ProbeWindowHandler::on_frame`] clears to a **hue that advances every frame** and
//! prints a frame counter to stderr every [`PROBE_LOG_EVERY`] frames. A static clear and a
//! dead render loop are indistinguishable on screen, and this repo has shipped exactly that
//! — #582's first cut wired `RedrawRequested` to `request_redraw()` instead of drawing, and
//! it compiled and passed the whole suite. With a ramp, "cycling, N frames" and "frozen at
//! 1" are *different observations* for whoever is sitting at the Mac, and
//! `clear_colour_changes_every_frame` makes it a test here.
//!
//! # Scope
//!
//! No `World` rendering, no egui, no input handling — those are #593 Tiers 1–3. One
//! question per probe. Nothing here touches `Shared` / IPC / `LAYOUT_VERSION`.
//!
//! # How to run it
//!
//! Gated twice — on the `mind-edition` feature (so the shipping plugin cdylib cannot move)
//! **and** on `ORGANON_EDITOR_PROBE=1`, checked at the top of `lib.rs`'s real `editor()`.
//! Deliberately not a separate binary: this is the actual host path Tier 2 has to work on
//! (`mind_main.rs` → `nih_export_standalone` → `Editor::spawn` with an `AppKitNsView`).
//!
//! ```sh
//! ORGANON_EDITOR_PROBE=1 ./organon-mind --backend dummy
//! ```
//!
//! `--backend dummy` is required: real CoreAudio hard-aborts the standalone (#579).

use std::any::Any;
use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use baseview::{
    Event, EventStatus, Size, Window, WindowEvent, WindowHandle, WindowHandler,
    WindowOpenOptions, WindowScalePolicy,
};
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use raw_window_handle_05::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle as RawDisplayHandle05,
    RawWindowHandle as RawWindowHandle05,
};
use wgpu::rwh as rwh06;

/// The environment variable that arms the probe. Checked in `lib.rs`'s `editor()`.
pub const PROBE_ENV: &str = "ORGANON_EDITOR_PROBE";

/// Logical size of the probe window. Small on purpose — the probe answers "does a frame
/// reach this view", and a big window only makes that slower to find out.
pub const PROBE_WIDTH: u32 = 640;
/// See [`PROBE_WIDTH`].
pub const PROBE_HEIGHT: u32 = 360;

/// Frames per full turn of the hue wheel. At baseview's ~66 Hz frame timer that is a
/// visible sweep of roughly eight seconds — slow enough to read as *motion* rather than
/// flicker, fast enough that a verifier does not have to wait to call it.
pub const HUE_PERIOD_FRAMES: u64 = 512;

/// How often `on_frame` prints its counter. 60 frames ≈ once a second.
pub const PROBE_LOG_EVERY: u64 = 60;

/// Whether the probe is armed, given the raw value of [`PROBE_ENV`].
///
/// Split out from the `std::env` read so the gate is testable without mutating process
/// environment from a parallel test runner.
pub fn probe_requested_from(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Whether the probe is armed in *this* process.
pub fn probe_requested() -> bool {
    probe_requested_from(std::env::var(PROBE_ENV).ok().as_deref())
}

// ─────────────────────────────────────────────────────────────────────────────
// The colour ramp
// ─────────────────────────────────────────────────────────────────────────────

/// Hue in `[0, 1)` for a given frame number.
pub fn probe_hue(frame: u64) -> f64 {
    (frame % HUE_PERIOD_FRAMES) as f64 / HUE_PERIOD_FRAMES as f64
}

/// Fully saturated HSV → RGB, value 1. Pure so the ramp is testable.
fn hue_to_rgb(hue: f64) -> [f64; 3] {
    let h = (hue.fract() + 1.0).fract() * 6.0;
    let sector = h.floor();
    let f = h - sector;
    let (r, g, b) = match sector as i32 % 6 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    [r, g, b]
}

/// The clear colour for a given frame — the whole visible payload of the probe.
pub fn probe_clear_colour(frame: u64) -> wgpu::Color {
    let [r, g, b] = hue_to_rgb(probe_hue(frame));
    wgpu::Color { r, g, b, a: 1.0 }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handle adaptation: nih-plug enum → rwh 0.5 → rwh 0.6
// ─────────────────────────────────────────────────────────────────────────────

/// nih-plug's [`ParentWindowHandle`] wearing baseview's raw-window-handle version.
///
/// nih-plug does implement rwh 0.5's `HasRawWindowHandle` for its own enum, but against
/// *its* `raw-window-handle` dependency; the moment that and baseview's diverge, the impl
/// is for a different trait and nothing compiles. `vendor/nih_plug_egui` carries the same
/// adapter for the same reason. Owning the conversion keeps the seam stable.
pub struct ParentWindowHandleAdapter(pub ParentWindowHandle);

unsafe impl HasRawWindowHandle for ParentWindowHandleAdapter {
    fn raw_window_handle(&self) -> RawWindowHandle05 {
        parent_window_handle_05(self.0)
    }
}

/// The body of the adapter, as a free function so every variant is testable.
pub fn parent_window_handle_05(parent: ParentWindowHandle) -> RawWindowHandle05 {
    match parent {
        ParentWindowHandle::X11Window(window) => {
            let mut handle = raw_window_handle_05::XcbWindowHandle::empty();
            handle.window = window;
            RawWindowHandle05::Xcb(handle)
        }
        ParentWindowHandle::AppKitNsView(ns_view) => {
            let mut handle = raw_window_handle_05::AppKitWindowHandle::empty();
            handle.ns_view = ns_view;
            RawWindowHandle05::AppKit(handle)
        }
        ParentWindowHandle::Win32Hwnd(hwnd) => {
            let mut handle = raw_window_handle_05::Win32WindowHandle::empty();
            handle.hwnd = hwnd;
            RawWindowHandle05::Win32(handle)
        }
    }
}

/// rwh 0.5 → rwh 0.6 for a **window** handle.
///
/// `None` means "wgpu cannot be pointed at this window": either a platform this build does
/// not target, or a handle whose payload is null/zero, which rwh 0.6 encodes as `NonNull` /
/// `NonZero` rather than accepting. Returning `Option` instead of `todo!()` (the reference
/// implementation in `egui-baseview` panics here) is the difference between a probe that
/// reports a failure and one that takes the host down with it.
pub fn window_handle_06(handle: RawWindowHandle05) -> Option<rwh06::RawWindowHandle> {
    Some(match handle {
        RawWindowHandle05::AppKit(h) => rwh06::RawWindowHandle::AppKit(
            rwh06::AppKitWindowHandle::new(NonNull::new(h.ns_view)?),
        ),
        RawWindowHandle05::Xlib(h) => {
            let mut out = rwh06::XlibWindowHandle::new(h.window);
            out.visual_id = h.visual_id;
            rwh06::RawWindowHandle::Xlib(out)
        }
        RawWindowHandle05::Xcb(h) => {
            let mut out = rwh06::XcbWindowHandle::new(NonZeroU32::new(h.window)?);
            out.visual_id = NonZeroU32::new(h.visual_id);
            rwh06::RawWindowHandle::Xcb(out)
        }
        RawWindowHandle05::Win32(h) => {
            let mut out = rwh06::Win32WindowHandle::new(NonZeroIsize::new(h.hwnd as isize)?);
            out.hinstance = NonZeroIsize::new(h.hinstance as isize);
            rwh06::RawWindowHandle::Win32(out)
        }
        _ => return None,
    })
}

/// rwh 0.5 → rwh 0.6 for a **display** handle. See [`window_handle_06`] on `None`.
///
/// This is the half that cannot be sourced from `ParentWindowHandle`: nih-plug's
/// `X11Window(u32)` is a bare window id with no server connection attached, and both
/// `XlibDisplayHandle` and `XcbDisplayHandle` want the connection pointer.
pub fn display_handle_06(handle: RawDisplayHandle05) -> Option<rwh06::RawDisplayHandle> {
    Some(match handle {
        RawDisplayHandle05::AppKit(_) => {
            rwh06::RawDisplayHandle::AppKit(rwh06::AppKitDisplayHandle::new())
        }
        RawDisplayHandle05::Xlib(h) => rwh06::RawDisplayHandle::Xlib(
            rwh06::XlibDisplayHandle::new(NonNull::new(h.display), h.screen),
        ),
        RawDisplayHandle05::Xcb(h) => rwh06::RawDisplayHandle::Xcb(rwh06::XcbDisplayHandle::new(
            NonNull::new(h.connection),
            h.screen,
        )),
        RawDisplayHandle05::Windows(_) => {
            rwh06::RawDisplayHandle::Windows(rwh06::WindowsDisplayHandle::new())
        }
        _ => return None,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// The window handler
// ─────────────────────────────────────────────────────────────────────────────

/// Everything downstream of a successful surface. Absent when GPU bring-up failed — the
/// window still opens and still counts frames in that case, because "the window came up but
/// the surface did not" and "nothing happened at all" are different answers and the probe
/// exists to tell them apart.
struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    /// Build instance → surface → adapter → device from a live baseview window.
    ///
    /// Mirrors `bin/visual.rs`'s negotiation, minus everything the probe does not draw: no
    /// adapter-specific format features, no ray query, no raised bind-group limit.
    fn new(window: &Window, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::default();

        // The two handles, both taken from the **baseview** window.
        let raw_window = window_handle_06(window.raw_window_handle())
            .ok_or_else(|| String::from("baseview window handle is not usable by wgpu"))?;
        let raw_display = display_handle_06(window.raw_display_handle())
            .ok_or_else(|| String::from("baseview display handle is not usable by wgpu"))?;

        // SAFETY: both handles come from the baseview window opened just above, which this
        // handler is stored inside — baseview drops the handler as part of closing that
        // window, so the surface never outlives the view it was created from.
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

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("organon-editor-probe"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);

        eprintln!(
            "[editor probe] surface up: {}×{} {:?} on {}",
            config.width,
            config.height,
            format,
            adapter.get_info().name
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if (w, h) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    /// Clear to `colour` and present. `false` = the swapchain needs rebuilding.
    fn clear(&mut self, colour: wgpu::Color) -> bool {
        // wgpu 30 returns an enum, not a `Result` — same acquire handling as
        // `bin/visual.rs`, including `Occluded` (a hidden or covered window, which
        // reconfiguring cannot fix).
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                f
            }
            wgpu::CurrentSurfaceTexture::Occluded => return true,
            _ => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("organon-editor-probe"),
            });
        // The whole payload: one clear, no draw calls. Dropped immediately — the pass
        // exists for its `LoadOp::Clear`.
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("organon-editor-probe clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(colour),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        }));
        self.queue.submit(Some(encoder.finish()));
        // wgpu 30 presents through the queue, not the texture (`world.rs::present` does the
        // same). This is the call the whole probe exists to reach.
        self.queue.present(frame);
        true
    }
}

/// The probe's `baseview::WindowHandler` — exactly the two methods baseview asks for.
struct ProbeWindowHandler {
    gpu: Option<Gpu>,
    frames: u64,
}

impl ProbeWindowHandler {
    fn new(window: &mut Window) -> Self {
        // Logical size, deliberately: baseview exposes no size getter, and on macOS it
        // starts a `SystemScaleFactor` window at scale 1.0 and only learns the real backing
        // scale when the view is laid out — at which point it fires `Resized` with the
        // physical size and `on_event` reconfigures. So on a Retina display the very first
        // frames are a logical-sized drawable scaled up by Core Animation, which for a flat
        // clear is invisible. Tier 2 draws geometry and must take its size from
        // `WindowInfo::physical_size` throughout.
        let gpu = match Gpu::new(window, PROBE_WIDTH, PROBE_HEIGHT) {
            Ok(gpu) => Some(gpu),
            Err(e) => {
                // Loud, not fatal: a host that loses its editor tells you far less than a
                // window that opens and says why it is blank.
                eprintln!("[editor probe] GPU bring-up FAILED: {e}");
                None
            }
        };
        Self { gpu, frames: 0 }
    }
}

impl WindowHandler for ProbeWindowHandler {
    fn on_frame(&mut self, _window: &mut Window) {
        self.frames += 1;
        let colour = probe_clear_colour(self.frames);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.clear(colour);
        }
        if self.frames % PROBE_LOG_EVERY == 0 {
            eprintln!(
                "[editor probe] frame {} — hue {:.3} rgb({:.2} {:.2} {:.2}){}",
                self.frames,
                probe_hue(self.frames),
                colour.r,
                colour.g,
                colour.b,
                if self.gpu.is_some() {
                    ""
                } else {
                    " [NO SURFACE]"
                },
            );
        }
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        if let Event::Window(WindowEvent::Resized(info)) = event {
            if let Some(gpu) = self.gpu.as_mut() {
                let size = info.physical_size();
                gpu.resize(size.width, size.height);
            }
        }
        // The probe handles no input (#593 Tier 3 does). Everything goes back to the host so
        // keyboard playthrough in a DAW keeps working while the probe is up.
        EventStatus::Ignored
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The Editor
// ─────────────────────────────────────────────────────────────────────────────

/// A second `nih_plug::editor::Editor` that does **not** go through `nih_plug_egui`.
///
/// See the module docs. Holds no GPU state itself: everything is built inside `spawn` and
/// dropped with the window. The *intent* of that arrangement is that a re-create would be a
/// fresh, independent bring-up rather than a resurrection of stale handles — but **no run
/// has ever performed a re-create**, so treat it as the design's claim about itself and not
/// as a verified property. Repeated bring-up and clean teardown are measured; the cycle is
/// not. `MIND_ARCHITECTURE.md` §2.4 carries the open question.
#[derive(Default)]
pub struct ProbeEditor {
    /// The host's DPI scale factor as `f32::to_bits`, or `0` for "never set". A scale of
    /// `0.0` is meaningless, so the sentinel is unambiguous.
    scale_bits: AtomicU32,
    /// Shared with the handle so `Drop` can clear it.
    open: Arc<AtomicBool>,
}

impl ProbeEditor {
    fn scaling_factor(&self) -> Option<f32> {
        match self.scale_bits.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f32::from_bits(bits)),
        }
    }
}

impl Editor for ProbeEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        _context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let window = Window::open_parented(
            &ParentWindowHandleAdapter(parent),
            WindowOpenOptions {
                title: String::from("Organon — editor probe"),
                size: Size::new(PROBE_WIDTH as f64, PROBE_HEIGHT as f64),
                // Same policy as `vendor/nih_plug_egui`: honour the host's factor when it
                // gave one, otherwise let the platform decide (macOS never sets it).
                scale: self
                    .scaling_factor()
                    .map(|f| WindowScalePolicy::ScaleFactor(f as f64))
                    .unwrap_or(WindowScalePolicy::SystemScaleFactor),
                // baseview's `opengl` feature is on across the whole graph (the egui editor
                // needs it), so this field always exists. `None` = no GL context: the probe
                // draws with wgpu, which is the entire point.
                gl_config: None,
            },
            ProbeWindowHandler::new,
        );
        self.open.store(true, Ordering::Release);
        eprintln!("[editor probe] spawned — expect a cycling clear colour and a frame count");
        Box::new(ProbeEditorHandle {
            open: self.open.clone(),
            window,
        })
    }

    fn size(&self) -> (u32, u32) {
        (PROBE_WIDTH, PROBE_HEIGHT)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // While the window is up there is nowhere to put a new scale factor, so refuse it —
        // Ableton does try this. Mirrors `nih_plug_egui`.
        if self.open.load(Ordering::Acquire) {
            return false;
        }
        self.scale_bits.store(factor.to_bits(), Ordering::Relaxed);
        true
    }

    // The probe draws no parameters, so parameter traffic is nothing to it. It repaints
    // unconditionally on baseview's frame timer.
    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}
    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
    fn param_values_changed(&self) {}
}

/// The handle nih-plug's wrapper holds for as long as the editor is open. Dropping it is
/// what closes the window — and therefore what drops the `Surface` before the view it was
/// created from goes away.
struct ProbeEditorHandle {
    open: Arc<AtomicBool>,
    window: WindowHandle,
}

// SAFETY: identical to `nih_plug_egui`'s. `WindowHandle` is `!Send` because it wraps raw
// platform pointers; nih-plug's wrapper only ever moves this box between threads, and only
// touches the window from the GUI thread that spawned it.
unsafe impl Send for ProbeEditorHandle {}

impl Drop for ProbeEditorHandle {
    fn drop(&mut self) {
        self.open.store(false, Ordering::Release);
        // Explicit, for the same reason nih_plug_egui does it: dropping the handle does not
        // reliably close the window on its own.
        self.window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    // ── the env gate ────────────────────────────────────────────────────────

    #[test]
    fn probe_gate_needs_exactly_one() {
        assert!(probe_requested_from(Some("1")));
    }

    #[test]
    fn probe_gate_rejects_everything_else() {
        for v in [None, Some(""), Some("0"), Some("true"), Some("yes"), Some("2")] {
            assert!(!probe_requested_from(v), "{v:?} should not arm the probe");
        }
    }

    // ── the ramp: the load-bearing part ─────────────────────────────────────

    /// The one that would have caught #582's black screen. A dead render loop and a static
    /// clear look identical on a screen; they do not look identical here.
    #[test]
    fn clear_colour_changes_every_frame() {
        for frame in 0..HUE_PERIOD_FRAMES {
            let a = probe_clear_colour(frame);
            let b = probe_clear_colour(frame + 1);
            assert!(
                (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs() > 1e-6,
                "frames {frame} and {} clear to the same colour",
                frame + 1
            );
        }
    }

    #[test]
    fn hue_wraps_at_the_period() {
        assert_eq!(probe_hue(0), 0.0);
        assert_eq!(probe_hue(HUE_PERIOD_FRAMES), probe_hue(0));
        assert_eq!(probe_hue(HUE_PERIOD_FRAMES + 7), probe_hue(7));
        assert!(probe_hue(HUE_PERIOD_FRAMES - 1) < 1.0);
    }

    #[test]
    fn clear_colour_stays_in_gamut() {
        for frame in 0..HUE_PERIOD_FRAMES * 2 {
            let c = probe_clear_colour(frame);
            for ch in [c.r, c.g, c.b] {
                assert!((0.0..=1.0).contains(&ch), "frame {frame} out of gamut: {c:?}");
            }
            assert_eq!(c.a, 1.0);
        }
    }

    /// A ramp that only ever moves one channel would still be a ramp, but a much worse
    /// signal to read across a room. Every channel must reach both rails.
    #[test]
    fn clear_colour_sweeps_every_channel() {
        let mut lo = [1.0f64; 3];
        let mut hi = [0.0f64; 3];
        for frame in 0..HUE_PERIOD_FRAMES {
            let c = probe_clear_colour(frame);
            for (i, ch) in [c.r, c.g, c.b].into_iter().enumerate() {
                lo[i] = lo[i].min(ch);
                hi[i] = hi[i].max(ch);
            }
        }
        for i in 0..3 {
            assert!(lo[i] < 0.01, "channel {i} never reaches black: {}", lo[i]);
            assert!(hi[i] > 0.99, "channel {i} never reaches full: {}", hi[i]);
        }
    }

    // ── nih-plug enum → rwh 0.5 ─────────────────────────────────────────────

    #[test]
    fn parent_handle_maps_every_variant() {
        let ns_view = 0x1234_usize as *mut c_void;
        match parent_window_handle_05(ParentWindowHandle::AppKitNsView(ns_view)) {
            RawWindowHandle05::AppKit(h) => assert_eq!(h.ns_view, ns_view),
            other => panic!("expected AppKit, got {other:?}"),
        }
        match parent_window_handle_05(ParentWindowHandle::X11Window(77)) {
            RawWindowHandle05::Xcb(h) => assert_eq!(h.window, 77),
            other => panic!("expected Xcb, got {other:?}"),
        }
        let hwnd = 0x4321_usize as *mut c_void;
        match parent_window_handle_05(ParentWindowHandle::Win32Hwnd(hwnd)) {
            RawWindowHandle05::Win32(h) => assert_eq!(h.hwnd, hwnd),
            other => panic!("expected Win32, got {other:?}"),
        }
    }

    // ── rwh 0.5 → rwh 0.6 ───────────────────────────────────────────────────
    //
    // Both rwh crates define every platform's handle types on *every* platform (that is why
    // `nih_plug_egui`'s adapter builds an `AppKitWindowHandle` on Linux and compiles), so
    // the path that will actually run — AppKit — is unit-testable from here rather than
    // merely type-checked by `cargo check --target aarch64-apple-darwin`.

    #[test]
    fn appkit_window_handle_survives_the_conversion() {
        let ns_view = 0xdead_beef_usize as *mut c_void;
        let mut src = raw_window_handle_05::AppKitWindowHandle::empty();
        src.ns_view = ns_view;
        match window_handle_06(RawWindowHandle05::AppKit(src)) {
            Some(rwh06::RawWindowHandle::AppKit(h)) => {
                assert_eq!(h.ns_view.as_ptr(), ns_view)
            }
            other => panic!("expected AppKit, got {other:?}"),
        }
    }

    /// rwh 0.6 stores the view as `NonNull`. A null `ns_view` must become a reported
    /// failure, not an `unwrap()` that takes the host down (the reference implementation in
    /// `egui-baseview` unwraps here).
    #[test]
    fn appkit_null_ns_view_is_refused() {
        let src = raw_window_handle_05::AppKitWindowHandle::empty();
        assert!(src.ns_view.is_null());
        assert!(window_handle_06(RawWindowHandle05::AppKit(src)).is_none());
    }

    #[test]
    fn appkit_display_handle_converts() {
        let src = raw_window_handle_05::AppKitDisplayHandle::empty();
        assert!(matches!(
            display_handle_06(RawDisplayHandle05::AppKit(src)),
            Some(rwh06::RawDisplayHandle::AppKit(_))
        ));
    }

    #[test]
    fn xcb_window_handle_survives_the_conversion() {
        let mut src = raw_window_handle_05::XcbWindowHandle::empty();
        src.window = 42;
        src.visual_id = 9;
        match window_handle_06(RawWindowHandle05::Xcb(src)) {
            Some(rwh06::RawWindowHandle::Xcb(h)) => {
                assert_eq!(h.window.get(), 42);
                assert_eq!(h.visual_id.map(|v| v.get()), Some(9));
            }
            other => panic!("expected Xcb, got {other:?}"),
        }
        // Window id 0 is "no window" in rwh 0.5 and unrepresentable in 0.6.
        assert!(window_handle_06(RawWindowHandle05::Xcb(
            raw_window_handle_05::XcbWindowHandle::empty()
        ))
        .is_none());
    }

    #[test]
    fn xlib_handles_survive_the_conversion() {
        let mut win = raw_window_handle_05::XlibWindowHandle::empty();
        win.window = 0xabc;
        match window_handle_06(RawWindowHandle05::Xlib(win)) {
            Some(rwh06::RawWindowHandle::Xlib(h)) => assert_eq!(h.window, 0xabc),
            other => panic!("expected Xlib, got {other:?}"),
        }

        let mut disp = raw_window_handle_05::XlibDisplayHandle::empty();
        disp.display = 0x1000_usize as *mut c_void;
        disp.screen = 3;
        match display_handle_06(RawDisplayHandle05::Xlib(disp)) {
            Some(rwh06::RawDisplayHandle::Xlib(h)) => {
                assert_eq!(h.display.map(|p| p.as_ptr()), Some(disp.display));
                assert_eq!(h.screen, 3);
            }
            other => panic!("expected Xlib display, got {other:?}"),
        }
    }

    #[test]
    fn win32_window_handle_survives_the_conversion() {
        let mut src = raw_window_handle_05::Win32WindowHandle::empty();
        src.hwnd = 0x99_usize as *mut c_void;
        src.hinstance = 0x11_usize as *mut c_void;
        match window_handle_06(RawWindowHandle05::Win32(src)) {
            Some(rwh06::RawWindowHandle::Win32(h)) => {
                assert_eq!(h.hwnd.get(), 0x99);
                assert_eq!(h.hinstance.map(|v| v.get()), Some(0x11));
            }
            other => panic!("expected Win32, got {other:?}"),
        }
        assert!(window_handle_06(RawWindowHandle05::Win32(
            raw_window_handle_05::Win32WindowHandle::empty()
        ))
        .is_none());
    }

    /// The whole chain the probe rests on, for the platform it will actually run on:
    /// nih-plug's enum → rwh 0.5 → rwh 0.6, pointer intact.
    #[test]
    fn appkit_parent_reaches_rwh_06_intact() {
        let ns_view = 0x5eed_usize as *mut c_void;
        let adapted = ParentWindowHandleAdapter(ParentWindowHandle::AppKitNsView(ns_view));
        match window_handle_06(adapted.raw_window_handle()) {
            Some(rwh06::RawWindowHandle::AppKit(h)) => {
                assert_eq!(h.ns_view.as_ptr(), ns_view)
            }
            other => panic!("expected AppKit, got {other:?}"),
        }
    }

    // ── the editor shell ────────────────────────────────────────────────────

    #[test]
    fn scale_factor_round_trips_and_is_refused_while_open() {
        let editor = ProbeEditor::default();
        assert_eq!(editor.scaling_factor(), None, "unset until the host says");
        assert!(editor.set_scale_factor(1.5));
        assert_eq!(editor.scaling_factor(), Some(1.5));

        editor.open.store(true, Ordering::Release);
        assert!(!editor.set_scale_factor(2.0), "must refuse while open");
        assert_eq!(editor.scaling_factor(), Some(1.5));
    }

    #[test]
    fn editor_reports_the_probe_size() {
        assert_eq!(ProbeEditor::default().size(), (PROBE_WIDTH, PROBE_HEIGHT));
    }
}

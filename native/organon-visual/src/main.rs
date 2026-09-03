//! Organon — separate-process visual window.
//!
//! A normal winit + wgpu app (owns its own main thread, so it can go fullscreen
//! cleanly). Reads the live parameter snapshot the plugin writes to shared
//! memory and renders the cube field every frame. Run it directly, or let the
//! plugin's "Open Visual Window" button launch it.
//!
//! # What is left here after the world hoist (#572 stage 2)
//!
//! The scene, the renderer, the generators, the beat clock, the camera, the recorder — all of
//! it moved to `src/world.rs` so the library (and therefore Organon Mind's editor) can drive
//! it. What remains is **the window**: creating it, picking the display it opens on, running
//! winit's event loop, and forwarding events to [`World`].
//!
//! `World` is a *library* type now, so `impl ApplicationHandler for World` is impossible here
//! — winit's trait, another crate's type. Hence [`VisualApp`], a wrapper whose only field is
//! the world. It forwards three calls; everything the old handler used to poke at directly
//! stayed with the world (see `world.rs`'s module docs).

// organon#49 T4c-ii — the world, reached through the crate that owns it.
//
// 📌 This was a `#[path = "../world.rs"]` include for a real reason, now retired: the
// library's `world` was gated on the editions and this binary ships in both, and a `#[path]`
// include is not a cargo feature, so including the SOURCE was how the binary got a world the
// cdylib did not. It cost a second compilation of the same 13.5k lines. Now that `world`
// lives in `organon-world` behind its own feature and this binary is its own package, the
// feature can simply be on here and off for the plugin — so the world is compiled once.
use organon_world::world;

// macOS EDR (true HDR) plumbing: the metal layer belongs to whoever owns the window, which is
// this binary since #572 stage 3. No-op stubs off macOS.
//
// On Windows `set_hdr_output` routes to `hdr_windows` instead, so the off-macOS stub in here has
// no caller there — hence the allow, scoped to that one build. The macOS and Linux builds still
// report this module honestly.
#[cfg_attr(windows, allow(dead_code))]
mod hdr_macos;

// Windows HDR output (organon#658 Tier 4) — the same job through wgpu 30's native colour-space
// and display-HDR APIs rather than a raw-DXGI island; `hdr_windows.rs` documents why that route
// won. `set_hdr_output` below picks between the two at compile time.
//
// The module is compiled on **every** platform on purpose: its two decision functions are pure
// and unit-tested here, so the interpretation of a display's numbers is covered by the Linux and
// macOS CI legs rather than only by a box nobody can run. Off Windows nothing calls them, hence
// the scoped allow — on Windows it is absent, so real rot still reports.
#[cfg_attr(not(windows), allow(dead_code))]
mod hdr_windows;

// The launch watchdog (organon#588): AppKit does not always deliver
// `applicationDidFinishLaunching:` to a process nothing activated, and winit gates `Resumed`
// — the event this file builds the window in — on exactly that. Same `#[path]` treatment as
// `hdr_macos.rs`: it belongs to the window owner, so it lives with the binary that owns one.
mod launch_macos;

// Which swapchain format to configure, decided from the surface's *current* capabilities
// (organon#237). Pure and unit-tested; `WindowSurface::configure` is its only caller and the
// only place the swapchain is configured.
mod surface_format;

use organon_core::ipc;
// organon#49 T4c-ii — the `use organon_core::math` shim that stood here is GONE, and the
// reason it existed is what retired it. `render.rs` and four of its submodules say
// `crate::math::…`; while this binary `#[path]`-included `world.rs`, `crate::` meant *this
// binary*, so the import was load-bearing even though nothing below named it. Those files
// live in `organon-render` now and resolve `crate::math` inside it. Removing this was
// checked by compiling, not by reading.
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::monitor::MonitorHandle;
#[cfg(target_os = "macos")]
use winit::platform::macos::EventLoopBuilderExtMacOS;
use winit::window::{Fullscreen, Window, WindowId};
use organon_world::egui_platform::WindowGeometry;
use world::{EventResponse, FrameTarget, World};

/// The window's geometry, as the UI layer's platform seam wants it stated (#593 Tier 3).
///
/// Both facts come off the same window `winit_platform::WinitPlatform` holds, which is why the
/// winit backend can let `egui-winit` read the window and the two cannot disagree. A baseview
/// host builds the same struct from the `WindowInfo` it kept.
fn geometry(window: &Window) -> WindowGeometry {
    let size = window.inner_size();
    WindowGeometry::new((size.width, size.height), window.scale_factor() as f32)
}

/// The window + swapchain — **the host's half of the seam** (#572 stage 3).
///
/// This used to be `world::WindowSurface`, a `Gfx` field. It moved out here because a surface
/// needs a window handle and the world is not allowed to want one: route C's editor builds its
/// surface on the `NSView` nih-plug hands it, with no winit anywhere in the process.
///
/// Everything in here is the standalone visual being *a* host. The world learns what it needs
/// per frame from `FrameTarget` and reports what it wants back in `FrameRequests`.
struct WindowSurface {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    /// Kept past `resumed` for one reason: `Surface::display_hdr_info` needs it, and the
    /// display's HDR state has to be re-read whenever the surface is reconfigured (#658 T4).
    /// Unused on the macOS path, which measures headroom off the `CAMetalLayer` instead.
    adapter: wgpu::Adapter,
    config: wgpu::SurfaceConfiguration,
    /// Whether the swapchain is **actually** the fp16 extended-linear HDR surface right now —
    /// the grant, as opposed to `hdr_applied`'s request (organon#237). Set by every
    /// successful `configure` from the format it chose and **cleared by a failed one**
    /// (`surface_format::Grant::after_configure`), and it is what gates the headroom read,
    /// the layer tag and the `HDR output: ON` line: a request that the surface could not
    /// honour leaves this `false`, `hdr_max` at `1.0` and the composite in its SDR arm.
    ///
    /// This replaces the `sdr_format` / `hdr_format` / `hdr_color_space` triple that was
    /// resolved **once** in `resumed`. That single read is what killed the visual twice on
    /// the workstation: on Vulkan the format list is a live answer about the display the
    /// window is on, and `Rgba16Float` leaves it when the display leaves HDR mode — so a
    /// reconfigure that re-issued the startup format was a validation error, and a
    /// validation error in `Surface::configure` is a panic. `surface_format.rs` has the
    /// full account.
    hdr_active: bool,
    /// Whether the last `configure` succeeded. A swapchain whose configure failed must not be
    /// acquired from — wgpu treats an acquire on an unconfigured surface as fatal — so the
    /// frame loop skips until a configure succeeds, rather than the process dying.
    configured: bool,
    /// The last configure error reported, so a persistent failure logs once rather than at
    /// every frame the loop retries in.
    last_configure_error: Option<String>,
    /// The HDR state currently *applied* to the swapchain + layer, so a change in the world's
    /// `hdr_request()` can be edge-detected. The world owns the intent (**H**, the editor
    /// checkbox); this is what actually got granted.
    ///
    /// ⚠️ It holds the **request**, deliberately, because it is compared against
    /// `hdr_request()` — storing a partly-refused grant here would make the two permanently
    /// unequal and re-run `sync_hdr` every frame. What was actually granted is `hdr_max` and
    /// `wide_granted` below.
    hdr_applied: (bool, bool),
    /// The display's measured EDR headroom, refreshed whenever EDR is (re-)asserted and handed
    /// to every frame. `1.0` = SDR.
    hdr_max: f32,
    /// Whether the surface is actually *tagged* wide-gamut, as opposed to merely asked to be.
    /// macOS grants every request (the layer takes `extendedLinearITUR_2020`); Windows cannot
    /// yet, and `hdr_windows::WIDE_GAMUT_GRANTED` explains why. The frame is told this rather
    /// than the request, so the composite's `hdr_vivid` never expands Rec.709 into a Rec.2020
    /// container the display never agreed to.
    wide_granted: bool,
}

/// What one [`WindowSurface::configure`] produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Configure {
    /// The swapchain is usable. `changed`: the format or the HDR grant differ from before
    /// the call — the caller re-reports the HDR state on it.
    Ok { changed: bool },
    /// `Surface::configure` raised an error. Already logged, never a panic; the swapchain
    /// must not be acquired from until a later configure succeeds.
    Failed,
}

impl WindowSurface {
    /// **The one place the swapchain is configured** (organon#237). First light, the HDR
    /// toggle, a resize and a lost/outdated acquire all arrive here, and the format is
    /// chosen from the capabilities read *at this call* — `surface_format.rs` owns the
    /// order and the reason `Rgb10a2Unorm` is not in it.
    ///
    /// `want_hdr` is the world's intent. The grant is what `hdr_active` says afterwards:
    /// fp16 is asked for only when the surface offers it **and** will present it in an
    /// extended-linear colour space (`hdr_output_color_space` — on Windows `Auto` on fp16
    /// can quietly resolve to plain sRGB, which would clamp the picture while every HDR
    /// control reported on). Anything less falls back to the sRGB surface the SDR path has
    /// always drawn into, and says so once (`fallback_line`) when the outcome moved or
    /// `announce` asks — the toggle announces; a resize that changes nothing is silent.
    ///
    /// The configure itself runs inside error scopes, because a validation error in
    /// `Surface::configure` is otherwise routed to the device's uncaptured-error handler,
    /// whose default is a panic — the exact death this fixes. A caught error is logged once
    /// (not per retried frame), `configured` goes false, and `render` skips frames until a
    /// configure succeeds; the process stays alive, which on a lock screen is the whole
    /// point.
    fn configure(&mut self, device: &wgpu::Device, want_hdr: bool, announce: bool) -> Configure {
        use wgpu::TextureFormat as F;
        let caps = self.surface.get_capabilities(&self.adapter);
        let fp16_offered = caps.formats.contains(&F::Rgba16Float);
        let hdr_space = if fp16_offered {
            hdr_output_color_space(caps.color_spaces(F::Rgba16Float))
        } else {
            None
        };
        let (format, hdr) =
            surface_format::pick_surface_format(want_hdr && hdr_space.is_some(), &caps.formats);
        let before = (self.config.format, self.hdr_active);
        self.config.format = format;
        // The colour space travels with the format (#658 T4). Off Windows both arms are
        // `Auto`, so the config there is bit-identical to what it was before the tier.
        self.config.color_space = match (hdr, hdr_space) {
            (true, Some(cs)) => cs,
            _ => wgpu::SurfaceColorSpace::Auto,
        };
        // The alpha mode was read at first light; a surface that has moved to another
        // adapter may not offer it any more, and an unsupported one is a validation error.
        if !caps.alpha_modes.contains(&self.config.alpha_mode) {
            self.config.alpha_mode = caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto);
        }
        // Three scopes, one per filter, popped in reverse: a configure error is a
        // validation error today, and the other two cost nothing to also catch.
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        self.surface.configure(device, &self.config);
        let e_internal = pollster::block_on(internal.pop());
        let e_oom = pollster::block_on(oom.pop());
        let e_validation = pollster::block_on(validation.pop());
        let err = e_internal.or(e_oom).or(e_validation);
        // The grant is a function of (what was asked for, whether the configure raised),
        // on BOTH paths: a failed configure clears `hdr_active` as well as `configured`, so
        // nothing downstream — `apply_hdr_output` through `sync_hdr`'s wide-gamut-only
        // branch, say — can read a stale grant and drive the platform HDR API over a
        // swapchain that no longer exists. Pure and tested in `surface_format.rs`.
        let grant = surface_format::Grant::after_configure(hdr, err.is_some());
        self.configured = grant.configured;
        self.hdr_active = grant.hdr_active;
        if let Some(err) = err {
            let msg = err.to_string();
            if self.last_configure_error.as_deref() != Some(msg.as_str()) {
                eprintln!(
                    "surface: configure failed for {format:?} {}x{} — drawing nothing until \
                     it succeeds. Offered: {:?}. {}",
                    self.config.width,
                    self.config.height,
                    caps.formats,
                    msg.trim(),
                );
                self.last_configure_error = Some(msg);
            }
            return Configure::Failed;
        }
        if self.last_configure_error.take().is_some() {
            eprintln!("surface: configure recovered ({format:?}).");
        }
        let changed = before != (format, hdr);
        if want_hdr && !hdr && (announce || changed) {
            eprintln!("{}", surface_format::fallback_line(&caps.formats, format, fp16_offered));
        }
        Configure::Ok { changed }
    }
}

/// The platform's HDR-output plumbing, chosen at compile time (organon#658 Tier 4).
///
/// Returns `(headroom, wide_gamut_granted)`: the display's headroom as a multiple of SDR white
/// (`1.0` = SDR, and the composite's SDR tone-map), and whether the surface really carries the
/// Rec.2020 tag `wide_gamut` asked for.
///
/// The two platforms reach the same pair of numbers by different roads — macOS through the
/// `CAMetalLayer` behind wgpu's back, Windows through wgpu's own surface colour space and DXGI
/// display query — and this is the seam where that stops mattering to the rest of the file.
/// **Everywhere else, including macOS, the behaviour is byte-for-byte what it was**: the
/// non-Windows arm is the same `hdr_macos::set_edr` call the call sites made directly, which is
/// the real EDR negotiation on the Mac and its own `1.0` no-op stub on Linux.
fn set_hdr_output(
    window: &Window,
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    enable: bool,
    wide_gamut: bool,
) -> (f32, bool) {
    #[cfg(windows)]
    {
        let _ = (window, wide_gamut);
        (
            hdr_windows::set_hdr_output(surface, adapter, enable),
            hdr_windows::WIDE_GAMUT_GRANTED,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (surface, adapter);
        (hdr_macos::set_edr(window, enable, wide_gamut), wide_gamut)
    }
}

/// The colour space to configure the `Rgba16Float` HDR swapchain with, or `None` when the
/// surface cannot present extended-range linear for it — in which case HDR is unavailable and
/// the swapchain stays SDR (organon#658 Tier 4).
///
/// `Some(Auto)` off Windows: that is the value this config has always carried, and on Metal
/// `Auto` resolves to `ExtendedSrgbLinear` for an fp16 surface anyway. Windows asks by name,
/// because there `Auto` can silently resolve to plain SDR `Srgb` instead — see
/// `hdr_windows::hdr_color_space`.
fn hdr_output_color_space(supported: wgpu::SurfaceColorSpaces) -> Option<wgpu::SurfaceColorSpace> {
    #[cfg(windows)]
    {
        hdr_windows::hdr_color_space(supported)
    }
    #[cfg(not(windows))]
    {
        let _ = supported;
        Some(wgpu::SurfaceColorSpace::Auto)
    }
}

/// winit's side of the seam: a [`World`] plus the window and swapchain it draws into.
struct VisualApp {
    world: World,
    /// `None` until `resumed` builds it.
    gfx: Option<WindowSurface>,
    /// The fullscreen state last applied, so `World::wants_fullscreen` is only acted on when it
    /// actually changes rather than on every event.
    fullscreen_applied: bool,
}

impl VisualApp {
    fn new() -> Self {
        VisualApp { world: World::new(organic_math_native::agent::core_catalog()), gfx: None, fullscreen_applied: false }
    }

    /// Re-assert HDR output and re-read the display's headroom. Cheap enough to call after any
    /// reconfigure or colorspace change — and it must follow `surface.configure`, which can
    /// reset the metal layer (macOS) and moves the swapchain to a new colour space (Windows).
    ///
    /// `on` is the **grant** — the request *and* an fp16 extended-linear swapchain actually
    /// configured (organon#237). Asking the layer for EDR, or reading a display's headroom,
    /// over an sRGB 8-bit swapchain would report a headroom the composite then re-expands
    /// highlights into, on a surface that clamps at 1.0.
    ///
    /// Was `apply_edr` until #658 Tier 4; "EDR" is macOS's word for it and this is no longer a
    /// macOS-only call.
    fn apply_hdr_output(&mut self) {
        let Some(g) = self.gfx.as_mut() else { return };
        let (want, wide) = self.world.hdr_request();
        let on = want && g.hdr_active;
        let (headroom, wide_granted) =
            set_hdr_output(&g.window, &g.surface, &g.adapter, on, wide);
        g.hdr_max = if on { headroom } else { 1.0 };
        g.wide_granted = on && wide && wide_granted;
    }

    /// **Every `Surface::configure` in this binary goes through here** (organon#237): first
    /// light, the HDR toggle, a resize, and a lost/outdated acquire. The format is chosen
    /// from the capabilities read *now* (`WindowSurface::configure`), the headroom / layer
    /// state is refreshed to match what was actually granted, and the `HDR output:` lines
    /// are printed when the outcome moved or the caller asks (`announce`, the toggle).
    ///
    /// Returns whether the swapchain is usable. `false` means the configure raised an error
    /// — already logged, never a panic — and the frame loop must not acquire until a later
    /// call succeeds.
    ///
    /// The renderer's composite/FX/temporal pipelines are deliberately *not* rebuilt here: the
    /// frame already rebuilds them whenever its target format differs from the last one
    /// (`world.rs`, `set_surface_format`), which is exactly what a format change here produces
    /// on the very next frame — and the same edge-detect is what lets a **mid-run** loss of
    /// fp16 land as a fallback rather than a pipeline/target mismatch: the target format
    /// reaches the frame from `config.format`, so the frame after a fallback configure draws
    /// with pipelines built for the fallback.
    fn configure_surface(&mut self, announce: bool) -> bool {
        let want = self.world.hdr_request();
        let (was_active, changed) = {
            let Some(dev) = self.world.device() else { return false };
            let Some(g) = self.gfx.as_mut() else { return false };
            let was_active = g.hdr_active;
            match g.configure(dev, want.0, announce) {
                Configure::Failed => return false,
                Configure::Ok { changed } => (was_active, changed),
            }
        };
        // Layer tag / headroom must follow `configure()`. Only when HDR is or was in force:
        // an SDR-only session never touches the metal layer, exactly as it never did.
        if was_active || self.gfx.as_ref().is_some_and(|g| g.hdr_active) {
            self.apply_hdr_output();
        }
        if announce || changed {
            self.report_hdr_output(want);
        }
        true
    }

    /// The `HDR output:` state lines, printed from the **grant** rather than the request, so
    /// they cannot claim EDR headroom while the fallback surface is in force. The fallback
    /// itself is announced by `WindowSurface::configure`, which is the only place that knows
    /// what the surface offered.
    fn report_hdr_output(&self, want: (bool, bool)) {
        let Some(g) = self.gfx.as_ref() else { return };
        if want.0 && g.hdr_active {
            eprintln!(
                "HDR output: ON — EDR headroom {:.2}× SDR white{}. \
                 (Needs a HDR display + OS HDR enabled to show extra range.)",
                g.hdr_max,
                if g.wide_granted { ", Rec.2020 wide gamut" } else { "" },
            );
            // Asked for the wide container and didn't get it. Say so once per toggle
            // rather than leaving the checkbox looking effective — on Windows this is
            // permanent for now (`hdr_windows::WIDE_GAMUT_GRANTED`).
            if want.1 && !g.wide_granted {
                eprintln!(
                    "HDR output: Rec.2020 wide gamut unavailable on this platform — \
                     output stays Rec.709 and hdr_vivid is inert."
                );
            }
        } else if !want.0 {
            eprintln!("HDR output: OFF (SDR / ACES).");
        }
        // `want.0 && !g.hdr_active`: the fallback line has just been printed by `configure`.
    }

    /// Swap the swapchain between the SDR sRGB surface and the `Rgba16Float` HDR one when the
    /// world's intent changes (**H**, or the editor's Renderer checkbox via IPC).
    ///
    /// A change of the *wide-gamut* bit alone re-tags the layer without touching the
    /// swapchain (the tag lives on the layer on macOS, and is refused outright on Windows);
    /// a change of the HDR bit reconfigures, through the one path every configure takes.
    fn sync_hdr(&mut self) {
        let want = self.world.hdr_request();
        let Some(g) = self.gfx.as_ref() else { return };
        if g.hdr_applied == want {
            return;
        }
        if g.hdr_applied.0 == want.0 {
            self.apply_hdr_output();
            self.report_hdr_output(want);
        } else {
            // A failed configure is already logged; `render` retries it every frame, with
            // whatever the request is by then, so nothing is lost by recording the intent.
            let _ = self.configure_surface(true);
        }
        if let Some(g) = self.gfx.as_mut() {
            g.hdr_applied = want;
        }
    }

    /// One presented frame: acquire, draw, apply what the frame asked for, present.
    ///
    /// This is the whole of what stage 3 moved out of the world. The acquire's
    /// `Occluded`/`Lost` handling in particular is a *surface* concern and now reads as one.
    fn render(&mut self) {
        self.sync_hdr();
        // A swapchain whose last configure failed is not acquired from — wgpu treats that as
        // fatal. Retry the configure (the surface may have come back) and skip the frame if
        // it still refuses; a lock-screen process that draws nothing beats one that dies.
        let configured = self.gfx.as_ref().is_some_and(|g| g.configured);
        if !configured && !self.configure_surface(false) {
            return;
        }
        let Some(g) = self.gfx.as_ref() else { return };
        let frame = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // wgpu 30: a hidden/covered window acquires as `Occluded` (29 blocked inside
            // nextDrawable instead). Reconfiguring won't un-occlude it — skip quietly.
            wgpu::CurrentSurfaceTexture::Occluded => return,
            // `Outdated` / `Lost` / `Timeout` / `Validation`: the surface moved under us — a
            // display change is the usual cause, and on Vulkan a display leaving HDR mode
            // arrives exactly here, with `Rgba16Float` gone from the format list. So this
            // reconfigure re-picks from the live capabilities rather than re-issuing the
            // format that was valid at startup (#237).
            _ => {
                let _ = self.configure_surface(false);
                return;
            }
        };
        let (size, format, hdr_max, wide) = {
            let g = self.gfx.as_ref().unwrap();
            (
                (g.config.width, g.config.height),
                g.config.format,
                g.hdr_max,
                // The gamut the surface actually carries, not the one requested (#658 T4).
                g.wide_granted,
            )
        };
        let window = self.gfx.as_ref().unwrap().window.clone();
        let requests = self.world.render_into(FrameTarget {
            texture: &frame.texture,
            size,
            format,
            presented: true,
            hdr_max,
            wide_gamut: wide,
            // #593 Tier 3: the frame states the interface's scale factor rather than lending
            // the window. `Some` unconditionally here — this host always has a window, and
            // whether an interface is actually drawn is `UiLayer::visible`'s call (**U**).
            ui_scale_factor: Some(window.scale_factor() as f32),
        });
        // What the frame asked the host to do. Both are advisory and both are window-shaped,
        // which is exactly why they are returned rather than performed.
        if let Some((w, h)) = requests.inner_size {
            let _ = window.request_inner_size(PhysicalSize::new(w, h));
        }
        if let Some(title) = requests.title {
            window.set_title(&title);
        }
        self.world.present(frame);
        // #554 T1 — after the window, not instead of it: the separate visual window is the
        // projector path and must be unaffected by whether anyone is mirroring it.
        // #593 Tier 4 — this binary is compiled in **both** editions, so the gate is here too.
        //
        // ⚠️ **But do not read it as what stops Mind mirroring.** This binary is only ever
        // *built* feature-off — `bundle.sh`/`deploy.sh` produce one visual and both products
        // run it, with `$ORGANON_IPC_NS` (runtime) choosing the namespace, so its `EDITION` is
        // permanently `Full` and the mirror code is always present in the visual Mind spawns.
        // What actually stops it is upstream: a mind-edition **editor** never stamps
        // `Shared.mindview[3]`, so `mirror_want` latches `false`, `pump_mirror` returns before
        // allocating anything, and the ring file is never created. This `cfg` is the belt to
        // that brace — it keeps a hypothetical `--features mind-edition` visual honest and,
        // more usefully, makes the call graph say so.
        #[cfg(not(feature = "mind-edition"))]
        self.world.pump_mirror_after_frame();
    }

    /// Push the world's fullscreen intent onto the real window, on change only.
    fn sync_fullscreen(&mut self) {
        let want = self.world.wants_fullscreen();
        if want == self.fullscreen_applied {
            return;
        }
        self.fullscreen_applied = want;
        if let Some(g) = self.gfx.as_ref() {
            g.window
                .set_fullscreen(want.then(|| Fullscreen::Borderless(None)));
        }
    }
}

impl ApplicationHandler for VisualApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Disarm the launch watchdog. Above the idempotence guard on purpose: a *second*
        // `Resumed` still means the app launched, which is the only thing the flag claims
        // (organon#588).
        launch_macos::mark_resumed();
        // Both halves of the seam must be idempotent: `resumed` can fire more than once, and
        // building a second device would leak the first.
        if self.gfx.is_some() || self.world.is_attached() {
            return;
        }
        let mut attrs = Window::default_attributes()
            .with_title("Organon — Visual")
            .with_inner_size(PhysicalSize::new(1100, 760));
        // #<projector>: open borderless-fullscreen directly on the projector when
        // one is attached. In James' live rig the laser projector shows up as a
        // second display, and the visual should land there maximized without
        // dragging + pressing F. `pick_launch_monitor` returns the target display
        // (the non-primary one by default, or whatever ORGANON_VISUAL_DISPLAY
        // pins); `None` = stay windowed as before (single display, or opted out).
        // Chosen at create time so the window never flashes windowed first.
        //
        // #554 Tier 4: **not** for an instrument window. Organon Mind's visual comes up
        // with an interface drawn in it, and seizing the second display on launch is
        // projector behaviour — it throws the instrument onto whatever is plugged in,
        // which on a non-HDR second display also silently costs it the EDR headroom the
        // UI layer's colour handling depends on. An explicit `ORGANON_VISUAL_DISPLAY`
        // still wins in both editions.
        let mut fullscreen = false;
        let instrument_window = organon_core::edition::EDITION.is_mind();
        if let Some(mon) = pick_launch_monitor(event_loop, instrument_window) {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(Some(mon))));
            fullscreen = true;
        }
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        self.fullscreen_applied = fullscreen;

        // --- The host's half of the seam (#572 stage 3) ----------------------
        // Instance, surface, adapter, feature/limit negotiation and swapchain config are
        // the window owner's. Everything downstream of the `Device` is the world's, and
        // is identical for every host — see `World::attach_gpu`.
        // #658 Tier 1 — `.with_env()`, so `WGPU_BACKEND` actually selects a backend.
        //
        // ⚠️ This was assumed to work already and does **not**: in wgpu 30 the environment is
        // read only by the `*_from_env` / `with_env` constructors, never by
        // `InstanceDescriptor::default()`. So `WGPU_BACKEND=dx12 organic-math-visual` was a
        // silent no-op — measured on the workstation, where it kept selecting Vulkan and the
        // log line below reported Vulkan while the operator believed they were testing DX12.
        // That matters beyond tidiness: the whole `rt_*` stack rides on `EXPERIMENTAL_RAY_QUERY`,
        // whose availability is a per-backend question, and this env var is the only way to ask
        // it of the other arm without a rebuild (#658 Tier 4).
        //
        // **Inert with no `WGPU_*` set** (invariant #4): `with_env()` calls `with_env` per field,
        // and each field keeps its existing value when its variable is absent — so an unset
        // environment reproduces `Instance::default()` exactly, on every platform.
        //
        // Scoped to the visual on purpose. `wgpu_editor.rs` and `editor_probe.rs` each build
        // their own `Instance`; those are UI devices, not the render device the RT question is
        // about, and neither has been exercised on this hardware yet (#658 Tier 2).
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
        // Request adapter-specific format features when present: the HDR scene
        // buffer is `Rgba16Float`, and the WebGPU spec only guarantees MSAA [1,4]
        // for it — 2× and 8× need TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES (Metal
        // supports [1,2,4,8] with it). Intersect with the adapter so this is a
        // no-op where unsupported.
        // Hardware ray tracing (#195 Tier 0): ray queries + acceleration
        // structures, hardware-accelerated on M3+ Apple GPUs via Metal. The
        // `adapter.features() & wanted` intersection below makes this a no-op
        // where unsupported — the device comes up exactly as before and
        // `RtContext::new` returns `None`.
        let wanted = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::EXPERIMENTAL_RAY_QUERY
            // GPU frame timing (#277 Tier 3). The timer writes timestamps on BARE
            // command encoders (outside any pass), which needs TIMESTAMP_QUERY plus
            // TIMESTAMP_QUERY_INSIDE_ENCODERS — plain TIMESTAMP_QUERY only permits
            // timestamps at pass boundaries. Intersected with the adapter below, so
            // it's a no-op where absent and the status bar falls back to CPU ms.
            | wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        // Neural acceleration (#200 Tier 2): detect (do NOT enable) the
        // cooperative-matrix fast path + f16. Enabling experimental coop-matrix on
        // the render device — and the GFLOPs microbenchmark that measures it — need
        // the Mac (the ray-query wedge showed experimental GPU features can wedge
        // the machine). Reporting availability lets the editor say what's possible.
        let coopmat_available = adapter
            .features()
            .contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);
        let f16_available = adapter.features().contains(wgpu::Features::SHADER_F16);
        // The cube pipeline uses FIVE bind groups (0..4: uniforms / IBL / RD scene /
        // GI probes / shadow map — #152 Tier 3), one more than wgpu's default
        // `max_bind_groups` cap of 4. Metal/Vulkan/DX12 all report ≥8, so raise the
        // requested limit to what the adapter actually offers (a no-op where the
        // default already suffices). Without this the `cube` shader module fails to
        // create at startup — "group index 4 exceeds the max_bind_groups limit of 4".
        let mut required_limits = wgpu::Limits::default();
        required_limits.max_bind_groups = adapter.limits().max_bind_groups;
        // wgpu gates EXPERIMENTAL_* features behind an explicit acknowledgement
        // token on top of the feature bit itself — requesting the feature without
        // it is a hard `RequestDeviceError::ExperimentalFeaturesNotEnabled`, even
        // when the adapter offers it. Only acknowledge when we actually request
        // an experimental bit, so a non-RT machine's device request is untouched.
        // SAFETY: the token is wgpu's "there may be UB-bugs in experimental APIs"
        // waiver; all our ray-query use is contained in rt.rs (#195's churn rule).
        let required_features = adapter.features() & wanted;
        let experimental_features =
            if required_features.intersects(wgpu::Features::all_experimental_mask()) {
                unsafe { wgpu::ExperimentalFeatures::enabled() }
            } else {
                wgpu::ExperimentalFeatures::disabled()
            };
        // The acceleration-structure limits default to 0 — a device that got the
        // ray-query feature still rejects any BLAS/TLAS creation unless they're
        // requested too. Mirror the adapter's caps (0 on non-RT machines: no-op).
        if required_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
            let al = adapter.limits();
            required_limits.max_blas_primitive_count = al.max_blas_primitive_count;
            required_limits.max_blas_geometry_count = al.max_blas_geometry_count;
            required_limits.max_tlas_instance_count = al.max_tlas_instance_count;
            required_limits.max_acceleration_structures_per_shader_stage =
                al.max_acceleration_structures_per_shader_stage;
        }
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("organic-math"),
                required_features,
                required_limits,
                experimental_features,
                ..Default::default()
            }),
        )
        .expect("request device");

        // #658 Tier 1 — say which adapter and backend we actually got. `Instance::default()`
        // picks silently, and until this line nothing in the visual reported the choice: on a
        // machine with both an iGPU and a discrete card, "HighPerformance" landing on the wrong
        // one looks identical to a slow scene. The backend half is the load-bearing part — the
        // whole RT stack rides on `EXPERIMENTAL_RAY_QUERY`, whose availability differs between
        // DX12 and Vulkan, and `WGPU_BACKEND=vulkan` is the one-line way to test the other arm.
        // Report the **granted** features, not the wanted set, so this cannot claim a capability
        // the device declined. One line at startup, no logger, matching `HDR output:` below.
        {
            let info = adapter.get_info();
            let got = device.features();
            let granted: Vec<&str> = [
                (wgpu::Features::EXPERIMENTAL_RAY_QUERY, "ray-query"),
                (wgpu::Features::TIMESTAMP_QUERY, "timestamp"),
                (wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS, "timestamp-in-encoder"),
                (wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES, "adapter-formats"),
            ]
            .into_iter()
            .filter(|(f, _)| got.contains(*f))
            .map(|(_, name)| name)
            .collect();
            eprintln!(
                "GPU: {} [{:?}, {:?}] driver: {} {} — granted: {}",
                info.name,
                info.backend,
                info.device_type,
                info.driver,
                info.driver_info,
                if granted.is_empty() {
                    "none of the optional features".to_string()
                } else {
                    granted.join(", ")
                },
            );
            // Detected-but-not-enabled (#200 Tier 2 keeps coop-matrix dark in the render loop).
            // Worth printing because it is exactly what #658 Tier 5 needs to know is reachable.
            eprintln!(
                "GPU: adapter also advertises: coop-matrix {}, shader-f16 {}",
                if coopmat_available { "yes" } else { "no" },
                if f16_available { "yes" } else { "no" },
            );
        }

        // The swapchain format is NOT chosen here (organon#237). `WindowSurface::configure`
        // chooses it from the capabilities it reads at each configure — this first one
        // included — because on Vulkan the list is a live property of the display the
        // window is on, and a choice made once at startup was what the visual died re-issuing.
        // `caps` is read here only for the alpha mode, which does not move.
        let caps = surface.get_capabilities(&adapter);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            // Placeholder, overwritten by the configure below before it reaches wgpu.
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            // wgpu 30: Auto reproduces 29's historical behavior exactly
            // (ExtendedSrgbLinear for Rgba16Float — the EDR path hdr_macos.rs
            // re-tags — else sRGB). `configure` swaps in the extended-linear space
            // alongside the fp16 format when HDR is granted, and `Auto` otherwise.
            // Native colour-space selection is how Windows does HDR as of #658
            // T4 (`hdr_windows.rs`); replacing hdr_macos.rs with it on the Mac is
            // a separate change that needs Mac verification, per #658's scope.
            color_space: Default::default(),
        };
        let mut ws = WindowSurface {
            window: window.clone(),
            surface,
            adapter,
            config,
            hdr_active: false,
            configured: false,
            last_configure_error: None,
            hdr_applied: (false, false),
            hdr_max: 1.0,
            wide_granted: false,
        };
        // First light is SDR, as it always was: `sync_hdr` flips to the HDR surface on the
        // first frame if the world already wants it. A failure here is logged by `configure`
        // and retried by `render` — not a panic.
        let _ = ws.configure(&device, false, false);
        let format = ws.config.format;

        // #554 Tier 4 / #593 Tier 3 — the UI layer, built by the *host* because only the host
        // knows its platform backend. This one is winit's; route C's editor builds the same
        // layer over baseview. Built for the surface's current format; an HDR swap rebuilds the
        // pipeline per frame via `set_format`.
        let ui = world::winit_platform::ui_layer(&device, window.clone(), format);
        self.world.attach_gpu(
            device,
            queue,
            format,
            Some(ui),
            coopmat_available,
            f16_available,
        );
        self.world.set_fullscreen_state(fullscreen);
        self.gfx = Some(ws);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.gfx.as_ref().map(|g| g.window.clone()) else { return };
        match self.world.on_window_event(geometry(&window), event) {
            EventResponse::Exit => {
                event_loop.exit();
                return;
            }
            // Draw NOW, not "ask for another redraw" — `request_redraw()` here would loop
            // `RedrawRequested` → request → `RedrawRequested` forever and never render a frame.
            // (That bug shipped in the first cut of this tier and was caught by the dead-code
            // warnings: with nothing calling `VisualApp::render`, the entire world tree went
            // unreachable and rustc reported 368 newly-dead items. The warning count was the
            // detector, which is the argument for not merging through a warning spike.)
            EventResponse::Redraw => self.render(),
            EventResponse::Continue => {}
        }
        self.sync_fullscreen();
        // A resize reconfigures the swapchain; `configure` can reset the metal layer, so EDR is
        // re-asserted afterwards. Read straight off the window rather than the event, so a
        // coalesced burst still lands on the final size.
        let inner = window.inner_size();
        let resized = self.gfx.as_mut().is_some_and(|g| {
            let moved = inner.width > 0
                && inner.height > 0
                && (g.config.width != inner.width || g.config.height != inner.height);
            if moved {
                g.config.width = inner.width;
                g.config.height = inner.height;
            }
            moved
        });
        if resized {
            // Through the one configure path (#237): a resize is also how a window arriving
            // on the other display, with other capabilities, first announces itself.
            let _ = self.configure_surface(false);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(g) = self.gfx.as_ref() {
            g.window.request_redraw();
        }
    }
}

/// Pick the display the visual should open fullscreen on at launch, or `None`
/// to stay windowed (the historical behaviour).
///
/// The live rig this serves: a laptop plus a laser projector wired in as a
/// *second* display. We want the visual to land maximized on the projector with
/// no dragging or F-key, and — crucially — to work even when it's launched from
/// the plugin's "Open Visual Window" button (a GUI child of the host, which does
/// **not** inherit the shell environment). So the *default* is zero-config
/// auto-detect, not an env var.
///
/// Where the visual should open, as a **pure decision over the spec string** — separated
/// from the `MonitorHandle` lookup so the policy is testable without a display server,
/// which is the only part of `pick_launch_monitor` that can be tested at all.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchDisplay {
    /// Stay windowed.
    Windowed,
    /// The automatic projector grab: the non-primary display, when 2+ are connected.
    AutoProjector,
    /// The primary display.
    Primary,
    /// A 1-based index into `available_monitors()` order.
    Index(usize),
    /// First display whose name contains this (already lowercased) substring.
    Named(String),
}

/// Classify `ORGANON_VISUAL_DISPLAY` into a [`LaunchDisplay`].
///
/// `instrument_window` is the #554 Tier 4 distinction: a window that comes up with an
/// interface in it is an instrument, not a projector feed, so the *automatic* grab is off
/// for it. Explicit specs are untouched by that — see [`pick_launch_monitor`].
fn launch_display(spec: &str, instrument_window: bool) -> LaunchDisplay {
    let spec = spec.trim().to_ascii_lowercase();
    match spec.as_str() {
        "off" | "none" | "windowed" | "false" | "0" => LaunchDisplay::Windowed,
        "" | "auto" => {
            if instrument_window {
                LaunchDisplay::Windowed
            } else {
                LaunchDisplay::AutoProjector
            }
        }
        "primary" => LaunchDisplay::Primary,
        other => match other.parse::<usize>() {
            Ok(n) => LaunchDisplay::Index(n),
            Err(_) => LaunchDisplay::Named(other.to_string()),
        },
    }
}

/// `ORGANON_VISUAL_DISPLAY` overrides, for solo dev and edge cases:
///   - unset / "auto"      → fullscreen on the non-primary display when 2+ are
///                            connected; windowed on a lone display.
///   - "off"/"none"/"windowed" → force windowed.
///   - a 1-based index ("1","2",…) → fullscreen on that display in
///                            `available_monitors()` order (1 = first).
///   - "primary"           → fullscreen on the primary display.
///   - any other text      → fullscreen on the first display whose name contains
///                            it (case-insensitive substring, e.g. a projector
///                            model string).
///
/// A single connected display with `auto` stays windowed on purpose: on the bare
/// laptop you want a normal window to drag around, not an inescapable fullscreen.
///
/// **`instrument_window` suppresses the *automatic* grab only** (#554 Tier 4). Since this
/// tier the visual's window is not always a projector feed: Organon Mind's comes up with
/// an interface in it, and a window you sit in front of must not seize the second display
/// on launch — that is projector behaviour, and it lands the instrument on whatever
/// happens to be plugged in. An **explicit** `ORGANON_VISUAL_DISPLAY` is still obeyed in
/// both editions, because naming a display is an instruction rather than a default.
fn pick_launch_monitor(
    event_loop: &ActiveEventLoop,
    instrument_window: bool,
) -> Option<MonitorHandle> {
    let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
    if monitors.is_empty() {
        return None;
    }
    let primary = event_loop.primary_monitor();
    let same = |a: &MonitorHandle, b: &MonitorHandle| a.position() == b.position();

    let spec = std::env::var("ORGANON_VISUAL_DISPLAY").unwrap_or_default();

    match launch_display(&spec, instrument_window) {
        LaunchDisplay::Windowed => None,
        LaunchDisplay::AutoProjector => {
            // Fullscreen on the projector (the non-primary display) only when more
            // than one display is present; otherwise leave the lone display windowed.
            if monitors.len() < 2 {
                return None;
            }
            match &primary {
                Some(p) => monitors.iter().find(|m| !same(m, p)).cloned(),
                // No known primary: fall back to the second enumerated display.
                None => monitors.get(1).cloned(),
            }
        }
        LaunchDisplay::Primary => primary.or_else(|| monitors.first().cloned()),
        LaunchDisplay::Index(n) => n.checked_sub(1).and_then(|i| monitors.get(i)).cloned(),
        LaunchDisplay::Named(name) => monitors
            .iter()
            .find(|m| {
                m.name()
                    .map(|n| n.to_ascii_lowercase().contains(&name))
                    .unwrap_or(false)
            })
            .cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- #554 T4: an instrument window must not seize the projector -------
    //
    // `pick_launch_monitor` itself needs a display server; the *policy* does not, and
    // the policy is where this can go wrong. The subtle half is not "Mind stays
    // windowed" — it is that suppressing the automatic grab must NOT also suppress an
    // explicit request, which is the easy over-correction.

    #[test]
    fn auto_grab_is_projector_only() {
        // The projector feed keeps the behaviour the live rig depends on...
        assert_eq!(launch_display("", false), LaunchDisplay::AutoProjector);
        assert_eq!(launch_display("auto", false), LaunchDisplay::AutoProjector);
        // ...and an instrument window never takes the second display by default.
        assert_eq!(launch_display("", true), LaunchDisplay::Windowed);
        assert_eq!(launch_display("auto", true), LaunchDisplay::Windowed);
    }

    #[test]
    fn an_explicit_display_is_obeyed_in_both_editions() {
        // Naming a display is an instruction, not a default — so Tier 4's suppression
        // must not swallow it. Getting this wrong would make ORGANON_VISUAL_DISPLAY
        // silently dead in Organon Mind.
        for instrument in [false, true] {
            assert_eq!(launch_display("primary", instrument), LaunchDisplay::Primary);
            assert_eq!(launch_display("2", instrument), LaunchDisplay::Index(2));
            assert_eq!(
                launch_display("Projector", instrument),
                LaunchDisplay::Named("projector".into()),
                "names are matched case-insensitively"
            );
        }
    }

    #[test]
    fn opting_out_still_wins_everywhere() {
        for instrument in [false, true] {
            for spec in ["off", "none", "windowed", "false", "0", "  OFF  "] {
                assert_eq!(
                    launch_display(spec, instrument),
                    LaunchDisplay::Windowed,
                    "spec {spec:?} should force windowed"
                );
            }
        }
    }
}

fn main() {
    // Black-box recorder: the plugin spawns this process with stderr lost, so
    // a panic was invisible (no .ips either — Rust panics exit cleanly). Write
    // every panic + backtrace to the namespaced sidecar before dying.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!(
            "{}\n\nbacktrace:\n{}",
            info,
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(ipc::panic_log_path(), &msg);
        default_hook(info);
    }));
    // The plugin spawns this process while the host (Ableton) is the frontmost
    // app. winit's default is `activateIgnoringOtherApps(true)` on launch, which
    // yanks the visual to the foreground — that deactivates Ableton and makes its
    // floating plugin-editor window disappear (the "VST UI auto-hides and I have
    // to click Ableton to get it back" gripe), and force-activating a freshly
    // spawned, non-bundled process is also the most likely cause of the
    // first-launch "have to press the button twice" flake. `false` = come up
    // WITHOUT stealing focus: the window still orders-front on the projector, but
    // Ableton stays active and its plugin editor stays put. The visual becomes
    // key normally the moment you click it (so F / Esc / O / H still work).
    let event_loop = {
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut builder = EventLoop::builder();
        #[cfg(target_os = "macos")]
        builder.with_activate_ignoring_other_apps(false);
        builder.build().expect("event loop")
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = VisualApp::new();
    // …and `false` is also why this next line has to exist. Coming up unactivated is
    // precisely the condition under which AppKit sometimes never delivers
    // `applicationDidFinishLaunching:` — and winit gates `Resumed` (and therefore the
    // window, and therefore everything) on it, so the process ends up an invisible
    // core-burning zombie. The watchdog delivers the callback itself if AppKit hasn't
    // within `GRACE`, which keeps the no-focus-stealing launch *and* the window.
    // See `launch_macos.rs` for the full autopsy (organon#588).
    launch_macos::arm();
    event_loop.run_app(&mut app).expect("run app");
}

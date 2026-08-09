//! **The UI layer** (#554 Tier 4) — egui drawn on the renderer's *own* wgpu device, into the
//! visual's own window, over the real scene.
//!
//! # What this changes
//!
//! Tier 1 put a picture of the scene inside the editor by copying it through system memory: the
//! visual rendered offscreen, read the texture back, and published RGBA8 into an mmap ring that
//! the editor uploaded with `ctx.load_texture`. That works, and it is still the right answer for
//! the **plugin inside Ableton**, where we do not own the window and have no business putting a
//! GPU device in the host's process.
//!
//! It cannot be the answer for Organon Mind, for two reasons that no amount of tuning fixes:
//!
//! - **It is not accelerated.** Every frame makes a round trip through system memory, and the
//!   readback stalls the render thread that produced it.
//! - **It cannot carry HDR.** `egui::ColorImage` is 8-bit sRGB. The visual's whole EDR path —
//!   the `Rgba16Float` swapchain, the extended-linear colorspace, the headroom shoulder — is
//!   quantised away at the copy. `FrameOutput::presented` already says as much: headroom is
//!   negotiated with a *display*, and an offscreen texture has none.
//!
//! Sharing the device removes both at once. The scene is already in the swapchain; egui simply
//! draws on top of it, in the same command encoder, at full precision.
//!
//! # Why this was believed impossible
//!
//! Because `egui-wgpu` has never published a version that accepts wgpu 30, and Organon's
//! renderer is on wgpu 30. That is a real constraint — cargo cannot resolve the pair — and it
//! was recorded in four places as the reason the frame mirror had to exist. What was never
//! measured is the **API gap behind the constraint**: in the file we keep it is eight
//! mechanical fixes (21 errors against the full crate, 13 of them in files this vendor drops).
//! `vendor/egui-wgpu` is that port, documented fix by fix.
//!
//! # Where this sits in the frame
//!
//! **After the composite.** `post.rs` owns exposure → bloom → tone-map → surface; the UI is
//! drawn into the swapchain *after* all of it, as a separate load-preserving pass. That
//! ordering is not incidental:
//!
//! - the UI must not be tone-mapped (a tone-mapped grey is not the grey the theme specified),
//! - the UI must not bloom (bright text would smear into the scene behind it),
//! - and the UI must not be exposed (an EV change would dim the controls along with the scene).
//!
//! The one thing the UI *does* have to respect is the target's encoding, which is the subtlest
//! part of this file's correctness and lives in `egui_wgpu::is_linear_target` — see the
//! `ORGANON PATCH (#554)` note there. The swapchain format changes underneath us whenever HDR
//! output is toggled, so [`UiLayer::set_format`] rebuilds the pipeline when it moves.
//!
//! # Input
//!
//! Routed through [`PointerRouter`], which has been sitting in `mind_shell.rs` since #541 —
//! written, unit-tested, and wired to nothing, because this is the consumer it was built for.
//! The property that matters is capture: **an orbit drag that starts on the scene stays with
//! the camera even when the pointer wanders over a panel**, and a drag that starts on a panel
//! never steals the camera. egui wins ties on hover, because its own hit-testing is the
//! authority on where its panels and popups actually are.
//!
//! **Where the events come from is no longer this file's business** (#593 Tier 3). It used to
//! be `egui-winit`, which is why `FrameTarget` carried a `&winit::Window` across the #572
//! window seam. The layer is now generic over [`EguiPlatform`]: the visual supplies the winit
//! arm ([`winit_platform`](super::winit_platform)), and route C's plugin-hosted editor supplies
//! a baseview one, where there is no winit anywhere in the process. The default type parameter
//! is the winit arm, so a host that has a window spells the type exactly as before.

use organic_math_native::egui_platform::{EguiPlatform, PointerPhase, WindowGeometry};
use organic_math_native::mind_shell::{PointerContext, PointerRouter, PointerTarget, Rect};

/// What the layer decided about one platform event.
///
/// Two answers, for two audiences: [`target`](Self::target) is the routing verdict the *world*
/// acts on (does this event belong to the camera or to a panel), and [`response`](Self::response)
/// is what the backend wants told to its *platform*. A winit host has nothing to tell one and
/// drops it; a baseview host must return an `EventStatus`, and getting that wrong is how a
/// plugin steals a DAW's transport or how a dropped file bounces back.
pub struct UiEvent<R> {
    /// Who owns the gesture.
    pub target: PointerTarget,
    /// The backend's platform answer, or `None` when the layer is hidden and egui never saw
    /// the event at all. `None` is not "egui declined" — it is "egui was not asked".
    ///
    /// ⚠️ **Nothing reads this today, and the `allow` is the honest way to say so.** The winit
    /// host has no platform to answer to: `ui_layer` discarded `egui_winit`'s `EventResponse`
    /// with a `let _` before this seam existed, and `consumed`/`repaint` tell a continuously
    /// redrawing window nothing the router has not already decided. The reader is route C's
    /// baseview host, where `EventResponse::status()` is what `WindowHandler::on_event` must
    /// return — and where dropping it means a DAW loses its space bar and a dropped `.gguf`
    /// bounces back (#599 documents both). Carrying it here is what stops that being a
    /// redesign later; the alternative is a seam that structurally cannot pass an
    /// `EventStatus` back.
    // Read by `wgpu_editor.rs` — in the LIBRARY. `bin/visual.rs` `#[path]`-includes this file
    // and its winit host has no reader by construction: it has nothing to report a status *to*
    // (#614's own note), so in that compilation the field is genuinely dead. The allow is
    // scoped to that fact rather than blanket — if the baseview reader ever goes away, this
    // should start warning in the library too.
    #[allow(dead_code)]
    pub response: Option<R>,
}

/// egui on the renderer's device: context, platform input, and the wgpu paint pass.
pub struct UiLayer<P: EguiPlatform = super::winit_platform::WinitPlatform> {
    ctx: egui::Context,
    platform: P,
    renderer: egui_wgpu::Renderer,
    /// The swapchain format the current pipeline was built for. HDR toggling changes it.
    format: wgpu::TextureFormat,
    router: PointerRouter,
    /// Whether the layer draws and takes input at all.
    ///
    /// **Off in full Organon, on in Organon Mind.** Full Organon's visual window *is* the
    /// projector feed — a HUD over it is exactly what a VJ does not want mid-set. Mind's window
    /// is an instrument you look at, so it comes up with its interface visible, the way any
    /// other 3-D application does. Either way **U** toggles it.
    pub visible: bool,
}

impl<P: EguiPlatform> UiLayer<P> {
    /// Build the layer against an existing device — the renderer's, never a second one.
    ///
    /// `ctx` and `platform` are taken rather than made because only the *host* knows which
    /// backend it has, and the two must share one `egui::Context`: the platform fills its
    /// `RawInput` and the layer runs it. `winit_platform::ui_layer` is the winit host's
    /// one-liner over this.
    pub fn new(
        device: &wgpu::Device,
        ctx: egui::Context,
        platform: P,
        format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            renderer: Self::build_renderer(device, format),
            ctx,
            platform,
            format,
            router: PointerRouter::new(),
            visible: organic_math_native::edition::EDITION.is_mind(),
        }
    }

    fn build_renderer(device: &wgpu::Device, format: wgpu::TextureFormat) -> egui_wgpu::Renderer {
        egui_wgpu::Renderer::new(
            device,
            format,
            egui_wgpu::RendererOptions {
                // The UI pass targets the swapchain directly, which is single-sample — the
                // scene's MSAA has already resolved by the time the composite runs. egui
                // feathers its own edges, so it needs no help here.
                msaa_samples: 1,
                // egui draws last and on top; there is nothing to depth-test against.
                depth_stencil_format: None,
                ..Default::default()
            },
        )
    }

    /// Hand the layer a wgpu texture to paint as an egui image, and get back the `TextureId`
    /// that names it (#617 Tier 1 — the workstation viewport).
    ///
    /// This is the seam that makes the scene a **widget**: the host renders the world into its
    /// own texture, registers it here once, and `editor_ui` paints it in the rect it reserved.
    /// Because egui owns the draw, the viewport clips, scrolls and lays out like anything else —
    /// which is the whole difference from immersive mode, where the world is painted straight
    /// onto the window and the interface floats over it.
    ///
    /// Pass the previous id back on every later frame. Registering afresh each frame would leak
    /// a bind group per frame; `update_egui_texture_from_wgpu_texture` rebinds the same id, which
    /// is what a resized scene texture needs.
    ///
    /// ⚠️ **`view` must sample *gamma-encoded* bytes** — egui's shader calls its sample
    /// `tex_gamma` and then linearizes it, so it assumes every texture it is given is already
    /// sRGB-encoded. A view that hands it linear values (or an `…Srgb` view, which the hardware
    /// decodes back to linear on sample) gets linearized a second time and the image comes out
    /// far too dark, with nothing erroring. `wgpu_editor`'s `SCENE_PANE_FORMAT` carries the
    /// two-view arrangement that satisfies this, and the measurement that caught it.
    pub fn register_scene_texture(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        previous: Option<egui::TextureId>,
    ) -> egui::TextureId {
        match previous {
            Some(id) => {
                self.renderer.update_egui_texture_from_wgpu_texture(
                    device,
                    view,
                    wgpu::FilterMode::Linear,
                    id,
                );
                id
            }
            None => {
                self.renderer.register_native_texture(device, view, wgpu::FilterMode::Linear)
            }
        }
    }

    /// Rebuild the pipeline if the swapchain format moved (the **H** key / the `hdr_output`
    /// param swap an `Rgba8UnormSrgb` surface for `Rgba16Float` and back).
    ///
    /// This is not an optimisation — the pipeline's colour target format is baked in at
    /// creation, so drawing into a format it was not built for is invalid. Getting it wrong
    /// would surface as a validation error at the exact moment someone presses **H**.
    pub fn set_format(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if needs_rebuild(self.format, format) {
            // **Rebuild the pipeline, do not replace the `Renderer`.** Replacing it drops the
            // texture map while our `egui::Context` still believes the font atlas is resident,
            // so the next atlas change arrives as an incremental delta against a texture that
            // no longer exists and `update_texture` panics ("Tried to update a texture that has
            // not been allocated yet"). That is not hypothetical — it is what H did on the Mac
            // with the panel up, and it survived the cloud session precisely because it needs
            // *both* HDR toggling and text on screen: with the UI hidden nothing lays out text,
            // no delta is ever emitted, and the toggle looks perfectly healthy.
            self.renderer.set_target_format(device, format);
            self.format = format;
        }
    }

    /// Feed a platform event to egui and decide who owns it.
    ///
    /// Returns the routing decision so the caller can act on scene-bound events and ignore
    /// UI-bound ones. When the layer is hidden, everything goes to the viewport and egui is
    /// never consulted — a hidden UI must not silently eat a drag.
    pub fn on_window_event(
        &mut self,
        geometry: WindowGeometry,
        event: &P::Event,
    ) -> UiEvent<P::Response> {
        if !self.visible {
            return UiEvent { target: PointerTarget::Viewport, response: None };
        }
        let response = self.platform.on_window_event(geometry, event);

        // **egui's `pixels_per_point`, not `geometry.scale_factor`.** The rect below is compared
        // against `pointer_latest_pos()`, which egui reports in its own points — display scale
        // times the context's zoom factor. Reading the display scale here instead would agree
        // at zoom 1.0 and put the router a zoom-factor out the moment anyone changes it.
        let ppp = self.ctx.pixels_per_point().max(f32::EPSILON);
        // The viewport is the whole window: under Shape B the scene *is* the window and the UI
        // floats over it, so there is no sub-rectangle to test against. `egui_wants_pointer`
        // carries the entire burden of "is the cursor over a panel", which is precisely the
        // question egui is authoritative on.
        let viewport = Rect {
            x: 0.0,
            y: 0.0,
            w: geometry.physical_size.0 as f32 / ppp,
            h: geometry.physical_size.1 as f32 / ppp,
        };
        let cx = PointerContext {
            egui_wants_pointer: self.ctx.wants_pointer_input(),
            pointer: self.ctx.pointer_latest_pos().map(|p| (p.x, p.y)),
            viewport,
        };

        let target = match P::pointer_phase(event) {
            PointerPhase::Press => self.router.on_press(cx),
            PointerPhase::Release => self.router.on_release(),
            PointerPhase::Move => self.router.on_move(cx),
            PointerPhase::Scroll => self.router.on_scroll(cx),
            // Keys go to egui whenever it has keyboard focus (a text field is active);
            // otherwise they stay with the visual's own shortcuts.
            PointerPhase::Other if self.ctx.wants_keyboard_input() => PointerTarget::Ui,
            PointerPhase::Other => PointerTarget::Nothing,
        };
        UiEvent { target, response: Some(response) }
    }

    /// Run the UI and paint it into `view`, on top of whatever is already there.
    ///
    /// `LoadOp::Load` is the load-bearing detail: the composite has already written the scene
    /// into this view, and the UI is drawn over it rather than replacing it.
    /// Owns its encoder and submits it, which is the convention every other late pass in
    /// `render_into` follows (`overlay::draw_hud_panel`, the RT debug blit).
    ///
    /// Returns whatever the backend could not perform itself (`()` for winit; a
    /// `PlatformActions` plan for a baseview host to apply where it holds its window), and
    /// `None` when the layer is hidden and no frame ran at all.
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        geometry: WindowGeometry,
        // `FnMut`, not `FnOnce`: egui may run the closure more than once in a frame
        // when a layout pass invalidates (a window that resizes to its content).
        mut build: impl FnMut(&egui::Context),
    ) -> Option<P::Deferred> {
        if !self.visible {
            return None;
        }
        let raw = self.platform.take_egui_input(geometry);
        let out = self.ctx.run(raw, |ctx| build(ctx));
        let deferred = self.platform.handle_platform_output(out.platform_output);

        let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let sd = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [geometry.physical_size.0, geometry.physical_size.1],
            pixels_per_point: out.pixels_per_point,
        };

        for (id, delta) in &out.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui-ui") });
        let mut staged = self.renderer.update_buffers(device, queue, &mut encoder, &jobs, &sd);

        {
            // Scoped so the pass drops — and so its recording ends — before `finish()`.
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-ui-layer"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                // The vendored renderer takes `RenderPass<'static>` (upstream's signature).
                .forget_lifetime();
            self.renderer.render(&mut rp, &jobs, &sd);
        }
        // `update_buffers` may hand back its own staging command buffers; they have to be
        // submitted *before* ours or the draws read buffers nothing has written yet.
        staged.push(encoder.finish());
        queue.submit(staged);

        // Free *after* submission: a texture egui dropped this frame is still referenced by
        // the draw calls just recorded.
        for id in &out.textures_delta.free {
            self.renderer.free_texture(id);
        }
        Some(deferred)
    }
}

/// The first real thing drawn on the shared device: a live state panel.
///
/// Deliberately small. This tier's claim is that **egui now renders on the renderer's own
/// device, correctly composited over the scene, with input routed** — and the honest way to
/// show that is real egui content (text layout, a texture atlas, a draggable window, theming)
/// rather than a coloured rectangle. Moving Organon Mind's workstation UI across is the next
/// step and a much larger one; it is not smuggled in here.
///
/// What it shows is chosen to be what someone verifying this on the Mac actually needs: which
/// generator is live, what the frame is costing, and — the part no test here can reach —
/// whether HDR is on and how much headroom the display gave us, since that is the exact
/// condition under which the UI's colour encoding could be wrong.
pub fn hud(ctx: &egui::Context, meta: HudState) {
    egui::Window::new(organic_math_native::edition::EDITION.product_name())
        .default_pos(egui::pos2(16.0, 16.0))
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            egui::Grid::new("organon-hud").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                ui.label("generator");
                ui.label(egui::RichText::new(&meta.generator).strong());
                ui.end_row();

                ui.label("frame");
                ui.label(format!("{:.1} ms  ({:.0} fps)", meta.frame_ms, meta.fps()));
                ui.end_row();

                ui.label("output");
                ui.label(format!("{} × {}", meta.size.0, meta.size.1));
                ui.end_row();

                ui.label("hdr");
                ui.label(if meta.hdr_enabled {
                    format!("on — {:.2}× headroom", meta.hdr_max)
                } else {
                    "off".to_string()
                });
                ui.end_row();
            });
            ui.separator();
            ui.label(egui::RichText::new("U hides this panel").weak().small());
        });
}

/// What [`hud`] draws. A plain snapshot so the panel never reaches into renderer state.
#[derive(Clone, Debug)]
pub struct HudState {
    pub generator: String,
    pub frame_ms: f32,
    pub size: (u32, u32),
    pub hdr_enabled: bool,
    pub hdr_max: f32,
}

impl HudState {
    /// Frames per second implied by the smoothed frame time, guarded so a zero-length first
    /// frame reports 0 rather than infinity.
    pub fn fps(&self) -> f32 {
        if self.frame_ms > 0.0 {
            1000.0 / self.frame_ms
        } else {
            0.0
        }
    }
}

/// Does a swapchain format change require rebuilding the egui pipeline?
///
/// Split out so the decision is testable without a device. A render pipeline's colour target
/// format is fixed at creation, so this is a correctness question, not a caching one.
pub fn needs_rebuild(current: wgpu::TextureFormat, incoming: wgpu::TextureFormat) -> bool {
    current != incoming
}

#[cfg(test)]
mod tests {
    use super::*;
    use organic_math_native::mind_shell::{PointerContext, PointerRouter, PointerTarget, Rect};

    fn viewport() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 1280.0, h: 860.0 }
    }

    #[test]
    fn an_unchanged_format_does_not_rebuild() {
        assert!(!needs_rebuild(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb
        ));
    }

    /// Pressing **H** swaps the surface between SDR and `Rgba16Float`. The pipeline is bound to
    /// its target format, so both directions must rebuild — missing either one is a validation
    /// error at the keypress, not at startup.
    #[test]
    fn toggling_hdr_rebuilds_in_both_directions() {
        let sdr = wgpu::TextureFormat::Rgba8UnormSrgb;
        let hdr = wgpu::TextureFormat::Rgba16Float;
        assert!(needs_rebuild(sdr, hdr));
        assert!(needs_rebuild(hdr, sdr));
    }

    /// The capture property, asserted against the real router this layer delegates to: a drag
    /// begun on the scene keeps the camera even once the pointer is over a panel that egui
    /// actively wants. Without this, an orbit that strays over a dock snaps the view.
    #[test]
    fn a_drag_begun_on_the_scene_keeps_the_camera_over_a_panel() {
        let mut r = PointerRouter::new();
        let start = PointerContext {
            egui_wants_pointer: false,
            pointer: Some((640.0, 400.0)),
            viewport: viewport(),
        };
        assert_eq!(r.on_press(start), PointerTarget::Viewport);

        let strayed = PointerContext { egui_wants_pointer: true, ..start };
        assert_eq!(r.on_move(strayed), PointerTarget::Viewport, "camera must keep the gesture");
        assert_eq!(r.on_scroll(strayed), PointerTarget::Viewport, "zoom follows the capture too");
        assert_eq!(r.on_release(), PointerTarget::Viewport);

        // Released — hover routing applies again and egui takes it back.
        assert_eq!(r.on_move(strayed), PointerTarget::Ui);
    }

    /// The converse, which is the half that keeps sliders usable: a drag begun on a panel never
    /// hands the camera a gesture, however far it strays over the scene.
    #[test]
    fn a_drag_begun_on_a_panel_never_steals_the_camera() {
        let mut r = PointerRouter::new();
        let on_panel = PointerContext {
            egui_wants_pointer: true,
            pointer: Some((40.0, 40.0)),
            viewport: viewport(),
        };
        assert_eq!(r.on_press(on_panel), PointerTarget::Ui);
        let over_scene = PointerContext { egui_wants_pointer: false, ..on_panel };
        assert_eq!(r.on_move(over_scene), PointerTarget::Ui);
        assert_eq!(r.on_release(), PointerTarget::Ui);
    }
}

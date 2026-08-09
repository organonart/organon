//! **The egui platform seam** (#593 Tier 3) — how egui gets its input, without naming a window.
//!
//! # The coupling this removes
//!
//! `ui_layer.rs` drove egui through `egui-winit`, and every one of those four calls wants a
//! `&winit::Window`. That is why `FrameTarget` carried a `ui_window: Option<&winit::Window>`
//! across the #572 window seam — the one coupling stage 3 could not remove, recorded in
//! `world.rs` as route C's to replace. This module is the replacement.
//!
//! # Why it is a trait over *data* and not over "a window"
//!
//! Because the two windows answer different questions.
//!
//! | | winit | baseview |
//! |---|---|---|
//! | `inner_size()` / `physical_size()` | yes | **no** |
//! | `scale_factor()` | yes | **no** |
//! | act on it (cursor, clipboard) | any time, through `&Window` | only inside a callback, `&mut Window` |
//!
//! `baseview::Window` is a handle you *act on*, not one you *ask*: geometry arrives only inside
//! `WindowEvent::Resized(WindowInfo)`, so the host keeps the latest one (it needs it for the
//! swapchain regardless) and states it. So the seam carries [`WindowGeometry`] — the two facts
//! egui actually reads off a window — rather than a window that may not be able to answer.
//!
//! Everything else follows from that one decision:
//!
//! - **[`EguiPlatform::pointer_phase`] classifies**, because `ui_layer` used to `match` on
//!   `winit::WindowEvent` variants to decide what to tell [`PointerRouter`][router], and only
//!   the backend knows its own event type.
//! - **[`EguiPlatform::handle_platform_output`] returns rather than acts.** It is the one call
//!   in the set with side effects, and the handle it needs is the one baseview will not lend
//!   outside a callback. A backend that holds its window does the work and returns `()`; one
//!   that does not returns its *plan* and its host applies it. That is not a workaround —
//!   `baseview_input::PlatformActions` was built for exactly this and its producer,
//!   `plan_platform_output`, is already public.
//!
//! The alternative was a generic associated type (`type Platform<'a> = baseview::Window<'a>`),
//! which does compile — at the cost of making the trait permanently dyn-hostile and putting a
//! `&mut ()` at the winit call site. Data out is symmetrical with data in, and cheaper.
//!
//! # Who implements it
//!
//! - **winit** — `world::winit_platform::WinitPlatform`, wrapping `egui_winit::State` plus the
//!   `Arc<Window>` it reads. Exercised for real by `organic-math-visual`, which *is* a winit
//!   host, so the visual's whole existing keymap and pointer behaviour is this arm's test.
//! - **baseview** — `baseview_input::State` (#599) satisfies this trait **unedited**; the arm
//!   is #593 Tier 2's, together with the window that produces the events.
//!
//! [router]: crate::mind_shell::PointerRouter

/// The window facts egui reads, as data.
///
/// The whole seam, in two fields. `egui_winit::State` gets these by asking its window;
/// `baseview_input::State` gets them from a [`baseview::WindowInfo`] the host kept — and
/// `WindowInfo` holds *exactly* these two (`physical_size()` and `scale()`), so the round trip
/// through this struct is lossless in both directions.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WindowGeometry {
    /// The drawable's size in **physical pixels**.
    pub physical_size: (u32, u32),
    /// Physical pixels per egui point — the display's scale factor. `1.0` on a non-Retina
    /// display, `2.0` on a Retina one.
    ///
    /// ⚠️ **This is the *display's* scale, not egui's `pixels_per_point`.** egui multiplies it
    /// by the context's zoom factor, so a backend feeding egui uses this as the *native* scale
    /// and anything working in egui's own coordinates must read `Context::pixels_per_point`
    /// instead. Mixing the two is a bug that only shows up once someone sets a zoom.
    pub scale_factor: f32,
}

impl WindowGeometry {
    pub const fn new(physical_size: (u32, u32), scale_factor: f32) -> Self {
        WindowGeometry { physical_size, scale_factor }
    }

    /// The scale factor, guarded against a value that cannot be divided by.
    ///
    /// A `0.0`, a negative, or a NaN scale — all three of which real backends have been seen to
    /// report while a window is being created or moved between displays — would send the logical
    /// size to infinity and lay out an interface that can never be drawn. Backends guard this
    /// themselves too (`baseview_input::State::resolve_pixels_per_point` does); this is the same
    /// guard at the seam, so a host cannot make the guarantee false for a backend that trusts it.
    pub fn scale(&self) -> f32 {
        if self.scale_factor.is_finite() && self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        }
    }

    /// The drawable's size in logical units (physical ÷ scale).
    pub fn logical_size(&self) -> (f32, f32) {
        let s = self.scale();
        (self.physical_size.0 as f32 / s, self.physical_size.1 as f32 / s)
    }
}

/// Which pointer gesture a platform event is, as far as routing is concerned.
///
/// This is `ui_layer`'s old `match` on `winit::WindowEvent`, lifted to a backend's decision.
/// It is deliberately coarse: [`PointerRouter`][router] only ever asks press / release / move /
/// scroll, and everything else — keys, focus, lifecycle — is [`Other`](Self::Other), which the
/// layer resolves against egui's keyboard focus exactly as it did before.
///
/// [router]: crate::mind_shell::PointerRouter
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerPhase {
    /// A pointer button went down.
    Press,
    /// A pointer button came up.
    Release,
    /// The pointer moved.
    Move,
    /// A scroll wheel or trackpad scroll.
    Scroll,
    /// Anything else — keyboard, focus, lifecycle, IME, touch.
    Other,
}

/// egui's platform integration, abstracted over the windowing backend (#593 Tier 3).
///
/// The four calls are `egui_winit::State`'s four, one for one, because the call site they serve
/// already exists and works — plus [`pointer_phase`](Self::pointer_phase), which used to be a
/// `match` in `ui_layer` and could not stay there once the event type stopped being winit's.
///
/// **Data in, data out.** The backend is the only thing in the design that touches a platform.
pub trait EguiPlatform {
    /// The backend's own event type — `winit::event::WindowEvent` / `baseview::Event`.
    type Event;

    /// What the backend reports back to its platform about one event.
    ///
    /// `egui_winit::EventResponse` for winit — which the visual ignores, because a winit host
    /// has nothing to report to. `baseview_input::EventResponse` for baseview, where it is
    /// load-bearing: its `accept_drop` is the gate the whole drag-and-drop gesture passes
    /// through, and `status()` turns it into the `EventStatus` baseview's `on_event` must
    /// return.
    type Response;

    /// What [`handle_platform_output`](Self::handle_platform_output) could **not** do itself.
    ///
    /// `()` where the backend holds its own window; `baseview_input::PlatformActions` where the
    /// platform only lends one inside a callback and the host has to apply the plan.
    type Deferred;

    /// Mirror of `egui_winit::State::take_egui_input` — the frame's accumulated input.
    fn take_egui_input(&mut self, geometry: WindowGeometry) -> egui::RawInput;

    /// Mirror of `egui_winit::State::on_window_event` — feed one platform event to egui.
    fn on_window_event(&mut self, geometry: WindowGeometry, event: &Self::Event) -> Self::Response;

    /// Mirror of `egui_winit::State::handle_platform_output` — cursor, clipboard, URLs.
    ///
    /// Returns what it could not do; see [`Deferred`](Self::Deferred).
    fn handle_platform_output(&mut self, output: egui::PlatformOutput) -> Self::Deferred;

    /// Classify an event for the pointer router. Pure and `self`-less on purpose: it is a table,
    /// it holds no state, and it is the one part of a backend that is fully testable offline.
    fn pointer_phase(event: &Self::Event) -> PointerPhase;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_divides_by_its_scale() {
        let g = WindowGeometry::new((2560, 1600), 2.0);
        assert_eq!(g.logical_size(), (1280.0, 800.0));
    }

    /// A scale factor that cannot be divided by falls back to 1.0 rather than producing an
    /// infinite or NaN layout. All three cases, because a backend has been seen to report each.
    #[test]
    fn an_undividable_scale_falls_back_to_one() {
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            let g = WindowGeometry::new((1100, 760), bad);
            assert_eq!(g.scale(), 1.0, "scale {bad} must not survive");
            assert_eq!(g.logical_size(), (1100.0, 760.0));
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // The trait is implementable by a backend that is neither winit nor baseview
    // ═════════════════════════════════════════════════════════════════════════

    /// A backend that holds no window at all, standing in for the one #593 Tier 2 writes.
    ///
    /// This exists to make the tier's central claim executable rather than asserted: *both
    /// hosts can satisfy this*. It records what the seam handed it (so the geometry contract is
    /// checked, not assumed) and defers its platform output the way a baseview backend must.
    #[derive(Default)]
    struct StubPlatform {
        seen: Vec<WindowGeometry>,
        events: usize,
    }

    /// What a backend that cannot touch its platform hands back — the shape of
    /// `baseview_input::PlatformActions`.
    #[derive(Debug, PartialEq)]
    struct StubActions {
        cursor: egui::CursorIcon,
    }

    impl EguiPlatform for StubPlatform {
        type Event = PointerPhase; // the stub's "platform event" is its own classification
        type Response = bool;
        type Deferred = StubActions;

        fn take_egui_input(&mut self, geometry: WindowGeometry) -> egui::RawInput {
            self.seen.push(geometry);
            let (w, h) = geometry.logical_size();
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            }
        }

        fn on_window_event(&mut self, geometry: WindowGeometry, _e: &PointerPhase) -> bool {
            self.seen.push(geometry);
            self.events += 1;
            true
        }

        fn handle_platform_output(&mut self, output: egui::PlatformOutput) -> StubActions {
            StubActions { cursor: output.cursor_icon }
        }

        fn pointer_phase(event: &PointerPhase) -> PointerPhase {
            *event
        }
    }

    /// The geometry a caller states reaches the backend unchanged — the property the whole
    /// design rests on, since a backend that cannot ask its window has no second source.
    #[test]
    fn the_seam_delivers_the_geometry_the_host_stated() {
        let mut p = StubPlatform::default();
        let g = WindowGeometry::new((2200, 1520), 2.0);

        assert!(p.on_window_event(g, &PointerPhase::Press));
        let raw = p.take_egui_input(g);

        assert_eq!(p.seen, vec![g, g], "both calls see exactly what the host stated");
        assert_eq!(
            raw.screen_rect.unwrap().size(),
            egui::vec2(1100.0, 760.0),
            "and it is enough to lay out a screen rect with no window in reach"
        );
    }

    /// Platform output crosses back as data, so a backend with no window handle in scope can
    /// still report what it wanted done. This is the half a GAT would have bought instead.
    #[test]
    fn platform_output_comes_back_as_data() {
        let mut p = StubPlatform::default();
        let out = egui::PlatformOutput { cursor_icon: egui::CursorIcon::Text, ..Default::default() };
        assert_eq!(
            p.handle_platform_output(out),
            StubActions { cursor: egui::CursorIcon::Text }
        );
    }

    /// Classification is a pure function of the event, callable with no backend instance —
    /// which is what lets a host's routing be tested without a window.
    #[test]
    fn pointer_phase_needs_no_instance() {
        assert_eq!(
            <StubPlatform as EguiPlatform>::pointer_phase(&PointerPhase::Scroll),
            PointerPhase::Scroll
        );
    }
}

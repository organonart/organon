//! #593 Tier 3 — **the baseview arm** of the [`EguiPlatform`] seam.
//!
//! Tier 3 (#614) defined the trait and wrote the winit arm; this is the other one. The split
//! was deliberate — the arm belongs to whoever owns the window that produces the events, and
//! that is Tier 2's editor (`wgpu_editor.rs`), whose window is the `NSView`/X11 child nih-plug
//! hands us. Two designs meeting in the middle is the failure mode the split exists to avoid.
//!
//! # #614's claim, verified
//!
//! #614 states that `baseview_input::State` (#599) satisfies [`EguiPlatform`] **unedited**, and
//! designed the trait so it would. **It holds**, and not one line of #599's translation changed
//! for this. Method for method:
//!
//! | [`EguiPlatform`] | `baseview_input::State` | |
//! |---|---|---|
//! | `take_egui_input(geometry)` | `take_egui_input(&WindowInfo)` | the impl converts; see below |
//! | `on_window_event(geometry, &Event) -> Response` | same, `-> EventResponse` | `Response = EventResponse`, so **`accept_drop` survives** — that field is the gate the whole drag gesture passes through on macOS/Windows |
//! | `handle_platform_output(output) -> Deferred` | **`plan_platform_output(output) -> PlatformActions`** | already `pub`, already "what the call would do, as data". This is the method #614 shaped the trait around, and it fits exactly |
//! | `pointer_phase(&Event)` | — | **the only new code here.** A pure table, like the winit arm's |
//!
//! Two notes on the fit, neither of which required changing #599:
//!
//! - **The geometry conversion is lossless.** [`WindowGeometry`] carries physical size + scale;
//!   `baseview::WindowInfo::from_physical_size` takes exactly that pair. #599 takes a
//!   `WindowInfo` because `baseview::Window` can answer neither question — it is a handle you
//!   act on, not one you ask — which is the same fact that made #614 put geometry in the trait.
//! - **`State` keeps an inherent `handle_platform_output(&mut Window, output)` as well.** The
//!   inherent one wins for method-call syntax and still works for a caller holding a window; the
//!   trait one is what generic code through `P` resolves to. Both are correct, and the trait's
//!   is the one `UiLayer` uses.
//!
//! # ⚠️ Why this is a library module, not a sibling of `winit_platform`
//!
//! `winit_platform.rs` lives inside `world.rs`'s `#[path]` tree, and the obvious symmetry is to
//! put this beside it. **The orphan rule forbids it.** That tree is compiled *twice* — once as
//! part of the library, and once inside `bin/visual.rs`, which `#[path]`-includes `world.rs`
//! because a binary cannot reach a library module gated behind `mind-edition`. In the *binary*
//! compilation both `EguiPlatform` and `State` are foreign types (they come from the
//! `organic_math_native` crate), so `impl EguiPlatform for State` there is an orphan impl and
//! does not build. `winit_platform` escapes this only because `WinitPlatform` is a **local
//! type** it defines itself.
//!
//! Implementing on `State` directly — rather than on a local newtype wrapping it — is what
//! keeps #614's "satisfies the trait unedited" claim literally true, so the impl belongs
//! wherever that is legal, which is here. The `UiLayer` constructor that would want
//! `super::ui_layer` lives in `wgpu_editor.rs` instead, which is inside the gate anyway.
//!
//! # Scope
//!
//! No `Shared`/IPC/`LAYOUT_VERSION` change — held through Tiers 0–3 and still holding.

use crate::baseview_input::{PlatformActions, State};
use crate::egui_platform::{EguiPlatform, PointerPhase, WindowGeometry};

/// [`WindowGeometry`] → the `WindowInfo` #599's translation reads.
///
/// `baseview` derives the logical size by dividing, so handing it the physical pair plus the
/// scale reproduces exactly what a real `Resized` event would have carried.
pub fn window_info(geometry: WindowGeometry) -> baseview::WindowInfo {
    baseview::WindowInfo::from_physical_size(
        baseview::PhySize {
            width: geometry.physical_size.0,
            height: geometry.physical_size.1,
        },
        geometry.scale() as f64,
    )
}

impl EguiPlatform for State {
    type Event = baseview::Event;
    /// **Load-bearing, unlike the winit arm's.** `EventResponse::status()` is what
    /// `WindowHandler::on_event` must return: `Ignored` hands the event back to the host (so a
    /// DAW keeps playing on the space bar), and `AcceptDrop` is the gate the whole drag gesture
    /// passes through — without it a dropped `.gguf` bounces back.
    type Response = crate::baseview_input::EventResponse;
    /// baseview only lends its window inside a callback, so the cursor/clipboard work comes
    /// back as a plan the host applies where it does hold one.
    type Deferred = PlatformActions;

    fn take_egui_input(&mut self, geometry: WindowGeometry) -> egui::RawInput {
        State::take_egui_input(self, &window_info(geometry))
    }

    fn on_window_event(
        &mut self,
        geometry: WindowGeometry,
        event: &Self::Event,
    ) -> Self::Response {
        State::on_window_event(self, &window_info(geometry), event)
    }

    fn handle_platform_output(&mut self, output: egui::PlatformOutput) -> Self::Deferred {
        // The plan, not the action — see the module docs.
        self.plan_platform_output(output)
    }

    /// The router's four gestures, off baseview's event enum. Pure and `self`-less, so it is
    /// fully testable here rather than only on a Mac.
    fn pointer_phase(event: &Self::Event) -> PointerPhase {
        match event {
            baseview::Event::Mouse(m) => match m {
                baseview::MouseEvent::ButtonPressed { .. } => PointerPhase::Press,
                baseview::MouseEvent::ButtonReleased { .. } => PointerPhase::Release,
                baseview::MouseEvent::CursorMoved { .. } => PointerPhase::Move,
                baseview::MouseEvent::WheelScrolled { .. } => PointerPhase::Scroll,
                _ => PointerPhase::Other,
            },
            _ => PointerPhase::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(e: baseview::Event) -> PointerPhase {
        <State as EguiPlatform>::pointer_phase(&e)
    }

    fn mouse(m: baseview::MouseEvent) -> baseview::Event {
        baseview::Event::Mouse(m)
    }

    /// The four gestures `PointerRouter` switches on. If any of these collapses to `Other` the
    /// router stops distinguishing UI from viewport and the camera eats every drag.
    #[test]
    fn the_four_router_gestures_map_across() {
        use baseview::{MouseButton, MouseEvent, Point, ScrollDelta};
        assert_eq!(
            phase(mouse(MouseEvent::ButtonPressed {
                button: MouseButton::Left,
                modifiers: Default::default()
            })),
            PointerPhase::Press
        );
        assert_eq!(
            phase(mouse(MouseEvent::ButtonReleased {
                button: MouseButton::Left,
                modifiers: Default::default()
            })),
            PointerPhase::Release
        );
        assert_eq!(
            phase(mouse(MouseEvent::CursorMoved {
                position: Point::new(1.0, 2.0),
                modifiers: Default::default()
            })),
            PointerPhase::Move
        );
        assert_eq!(
            phase(mouse(MouseEvent::WheelScrolled {
                delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
                modifiers: Default::default()
            })),
            PointerPhase::Scroll
        );
    }

    /// Keyboard and window events are not pointer gestures. Classifying one as `Move` would
    /// make the router re-decide ownership on a keystroke.
    #[test]
    fn non_pointer_events_are_other() {
        assert_eq!(
            phase(baseview::Event::Window(baseview::WindowEvent::WillClose)),
            PointerPhase::Other
        );
        assert_eq!(phase(mouse(baseview::MouseEvent::CursorLeft)), PointerPhase::Other);
    }

    /// The conversion the whole arm rests on: physical size and scale must survive intact, or
    /// egui lays out against the wrong rect on every HiDPI display.
    #[test]
    fn geometry_reaches_baseview_unchanged() {
        let g = WindowGeometry::new((1280, 860), 2.0);
        let info = window_info(g);
        assert_eq!(info.physical_size().width, 1280);
        assert_eq!(info.physical_size().height, 860);
        assert!((info.scale() - 2.0).abs() < 1e-9);
        // …and the logical size is the physical one divided by the scale, which is what egui
        // ultimately lays out against.
        assert!((info.logical_size().width - 640.0).abs() < 1e-9);
        assert!((info.logical_size().height - 430.0).abs() < 1e-9);
    }
}

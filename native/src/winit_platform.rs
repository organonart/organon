//! **The winit arm of the egui platform seam** (#593 Tier 3) — `egui-winit`, behind the trait.
//!
//! This is the half of Tier 3 that a winit host needs, and it is deliberately thin: every one
//! of `egui_winit::State`'s four calls goes straight through. What the trait buys is not a
//! different translation, it is that `ui_layer` no longer *names* winit — so the same layer
//! serves the plugin-hosted editor, whose events come from baseview and whose window can answer
//! neither of the questions `egui-winit` asks its own.
//!
//! # Why this backend ignores the geometry it is handed
//!
//! [`WindowGeometry`] is the seam's answer to a window that cannot be asked. `egui-winit`'s
//! window *can* be asked, and `take_egui_input` insists on doing so — it takes a `&Window` and
//! reads `inner_size()` and `scale_factor()` itself, with no way to pass values in. So this
//! backend keeps the `Arc<Window>` and lets `egui-winit` read it, exactly as `ui_layer` did
//! before the seam existed.
//!
//! **The two agree by construction**, which is the reason that is safe rather than merely
//! convenient: the host derives the geometry it states from the same window this holds
//! (`bin/visual.rs`'s `geometry()`). Restating that as an assertion would be checking winit
//! against itself.
//!
//! # Why it lives in the world's module tree
//!
//! Beside `ui_layer`, and gated with it. `egui-winit` is the visual binary's dependency, and
//! `world` is `mind-edition`-gated precisely so that egui-wgpu and a GPU device stay out of
//! Ableton's process in a default build. The trait itself is ungated in `lib.rs`, where the
//! baseview arm can reach it; only the arms live with their hosts.

use std::sync::Arc;

use organic_math_native::egui_platform::{EguiPlatform, PointerPhase, WindowGeometry};
use winit::event::WindowEvent;
use winit::window::Window;

/// `egui_winit::State` plus the window it reads.
pub struct WinitPlatform {
    state: egui_winit::State,
    /// Held rather than borrowed per call: holding it is what removes `&winit::Window` from
    /// `FrameTarget`, which is the whole deliverable of #593 Tier 3.
    window: Arc<Window>,
}

impl WinitPlatform {
    /// `ctx` must be the same context the [`UiLayer`](super::ui_layer::UiLayer) runs — egui's
    /// input state and its output are two halves of one conversation.
    pub fn new(ctx: egui::Context, window: Arc<Window>, max_texture_side: usize) -> Self {
        let state = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            window.as_ref(),
            // `None` = take the scale factor from the window, which is what we want on a
            // Retina display and on a projector alike.
            None,
            None,
            Some(max_texture_side),
        );
        WinitPlatform { state, window }
    }
}

impl EguiPlatform for WinitPlatform {
    type Event = WindowEvent;
    type Response = egui_winit::EventResponse;
    /// Nothing: this backend holds its window, so `handle_platform_output` performs the cursor,
    /// clipboard and URL work itself, the way `egui-winit` always has.
    type Deferred = ();

    fn take_egui_input(&mut self, _geometry: WindowGeometry) -> egui::RawInput {
        self.state.take_egui_input(&self.window)
    }

    fn on_window_event(
        &mut self,
        _geometry: WindowGeometry,
        event: &WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(&self.window, event)
    }

    fn handle_platform_output(&mut self, output: egui::PlatformOutput) {
        self.state.handle_platform_output(&self.window, output);
    }

    /// `ui_layer`'s old `match`, unchanged variant for variant.
    ///
    /// The catch-all really is everything else: keys, focus, IME, lifecycle and touch all
    /// resolve to [`PointerPhase::Other`], which the layer settles against egui's keyboard
    /// focus exactly as it did before. Touch is listed only to say it is not handled here and
    /// never was — winit reports it separately from the mouse, and this app has never had a
    /// touch path.
    fn pointer_phase(event: &WindowEvent) -> PointerPhase {
        match event {
            WindowEvent::MouseInput { state: winit::event::ElementState::Pressed, .. } => {
                PointerPhase::Press
            }
            WindowEvent::MouseInput { state: winit::event::ElementState::Released, .. } => {
                PointerPhase::Release
            }
            WindowEvent::CursorMoved { .. } => PointerPhase::Move,
            WindowEvent::MouseWheel { .. } => PointerPhase::Scroll,
            _ => PointerPhase::Other,
        }
    }
}

/// Build a UI layer driven by `window` — the winit host's one-liner.
///
/// Exists so `ui_layer.rs` never has to name `winit::Window` or `Arc` to offer a convenient
/// constructor, and so the three things that must line up (one `egui::Context`, shared between
/// the layer and its backend; the device's texture limit; the swapchain format) line up in one
/// place instead of at every host.
pub fn ui_layer(
    device: &wgpu::Device,
    window: Arc<Window>,
    format: wgpu::TextureFormat,
) -> super::ui_layer::UiLayer<WinitPlatform> {
    let ctx = egui::Context::default();
    let max_texture_side = device.limits().max_texture_dimension_2d as usize;
    let platform = WinitPlatform::new(ctx.clone(), window, max_texture_side);
    super::ui_layer::UiLayer::new(device, ctx, platform, format)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase};

    fn dev() -> DeviceId {
        DeviceId::dummy()
    }

    fn phase(e: WindowEvent) -> PointerPhase {
        <WinitPlatform as EguiPlatform>::pointer_phase(&e)
    }

    /// The four gestures `PointerRouter` acts on, mapped exactly as `ui_layer`'s `match` did
    /// before the seam. This is the table the visual's drag / orbit / zoom behaviour rides on,
    /// and the one thing about the winit arm a GPU-less environment *can* check.
    #[test]
    fn the_four_router_gestures_map_across() {
        assert_eq!(
            phase(WindowEvent::MouseInput {
                device_id: dev(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
            }),
            PointerPhase::Press
        );
        assert_eq!(
            phase(WindowEvent::MouseInput {
                device_id: dev(),
                state: ElementState::Released,
                button: MouseButton::Left,
            }),
            PointerPhase::Release
        );
        assert_eq!(
            phase(WindowEvent::CursorMoved {
                device_id: dev(),
                position: winit::dpi::PhysicalPosition::new(12.0, 34.0),
            }),
            PointerPhase::Move
        );
        assert_eq!(
            phase(WindowEvent::MouseWheel {
                device_id: dev(),
                delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                phase: TouchPhase::Moved,
            }),
            PointerPhase::Scroll
        );
    }

    /// Press and release must not collapse into one phase: the router latches capture on press
    /// and releases it on release, so a mapping that lost the distinction would leave an orbit
    /// drag stuck to the camera forever.
    #[test]
    fn press_and_release_stay_distinct() {
        let press = WindowEvent::MouseInput {
            device_id: dev(),
            state: ElementState::Pressed,
            button: MouseButton::Right,
        };
        let release = WindowEvent::MouseInput {
            device_id: dev(),
            state: ElementState::Released,
            button: MouseButton::Right,
        };
        assert_ne!(phase(press), phase(release));
    }

    /// Everything else is `Other`, which is what sends it to the layer's keyboard-focus rule.
    /// A key event landing on a router gesture would be a real bug — typing `u` into a text
    /// field would move the camera.
    #[test]
    fn everything_else_is_other() {
        assert_eq!(phase(WindowEvent::CloseRequested), PointerPhase::Other);
        assert_eq!(phase(WindowEvent::RedrawRequested), PointerPhase::Other);
        assert_eq!(phase(WindowEvent::Focused(true)), PointerPhase::Other);
        assert_eq!(
            phase(WindowEvent::Resized(winit::dpi::PhysicalSize::new(1100, 760))),
            PointerPhase::Other
        );
        assert_eq!(phase(WindowEvent::CursorLeft { device_id: dev() }), PointerPhase::Other);
    }
}

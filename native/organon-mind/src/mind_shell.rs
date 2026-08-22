//! **Organon Mind — the embedded workstation shell** (#532 Tier 1): the pure core.
//!
//! This module holds the parts of the shell that are *host-free and GPU-free*, so they
//! are unit-testable in a cloud session where no window can be opened:
//!
//! - [`layout_workstation`] — the dock/viewport partition (PRD §5.2). egui owns the
//!   panels; this decides how much room they get and what rectangle is left for the
//!   renderer.
//! - [`PointerRouter`] — who gets the mouse: egui, or the viewport camera.
//!
//! The winit/wgpu/egui event loop that consumes both is binary-only and lives outside
//! the cdylib, exactly like `render.rs` (see `MIND_ARCHITECTURE.md`).
//!
//! # ⚠️ The param bridge is BLOCKED — read this before trying again
//!
//! #532 Tier 1 specified a `MindGuiContext`: implement nih-plug's `GuiContext` ourselves
//! so the editor's 1421 `setter.*` call sites keep working outside a plugin host. The
//! trait is public and *implementable*, but such an implementation **cannot actually move
//! a parameter**, because every write path is sealed inside nih-plug:
//!
//! - `Param::set_normalized_value` / `update_smoother` are **not** on the public `Param`
//!   trait. They live on `pub(crate) trait ParamMut` (`nih-plug/src/params.rs:195`).
//! - `ParamPtr::set_normalized_value` is `pub(crate)` (`params/internals.rs:77`).
//! - The concrete types (`FloatParam`, …) expose only builders and `value()`.
//! - `standalone::Wrapper` *does* have a public `set_parameter(ParamPtr, f32)`, but the
//!   module is private — `wrapper/standalone.rs` exports only `nih_export_standalone`,
//!   so the type is unreachable from outside the crate.
//! - `wrapper::state`'s (de)serialization helpers are `pub(crate)` too.
//!
//! This is deliberate on nih-plug's part: parameter writes are the wrappers' job. So the
//! shell cannot host the existing editor UI until that is resolved — by a small patch to
//! our nih-plug fork (it is already a git dependency), by hosting our surface inside the
//! window nih-plug hands to `Editor::spawn` (where a genuine `GuiContext` *is* provided),
//! or by decoupling the editor from nih-plug params altogether. That is a project-level
//! decision, not something to pick here.
//!
//! **What is deliberately NOT here:** the scene. Producing a `render::RenderFrame` for
//! the specimen is today entangled in `bin/visual.rs`'s ~7100-line `App::render`, and
//! carving that into a reusable world is its own tier — not something to rush blind on a
//! machine with no GPU.

// ═════════════════════════════════════════════════════════════════════════════
// The dock / viewport partition
// ═════════════════════════════════════════════════════════════════════════════

/// A rectangle in **logical (egui) points**, origin top-left.
///
/// Deliberately not `egui::Rect`: this math is the part worth testing, and keeping it in
/// plain floats means the tests need no egui context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn area(&self) -> f32 {
        self.w * self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// Requested sizes for the surrounding docks (PRD §5.2), in logical points.
#[derive(Clone, Copy, Debug)]
pub struct DockSizes {
    /// Top strip: model / token counts / run state.
    pub top: f32,
    /// Left "Project" dock: model, parsed architecture, sessions.
    pub left: f32,
    /// Right "Properties" dock: lens params, selected element, provenance legend.
    pub right: f32,
    /// Bottom analytics dashboard (#482).
    pub bottom: f32,
    /// Status bar along the very bottom.
    pub status: f32,
}

impl Default for DockSizes {
    fn default() -> Self {
        // Chosen to match the reference mock at ~1680×940: docks present but the
        // viewport still clearly the subject.
        Self { top: 44.0, left: 260.0, right: 300.0, bottom: 220.0, status: 26.0 }
    }
}

/// The full partition of the window.
///
/// Every field is disjoint and together they exactly tile the window — asserted by test,
/// because a one-pixel overlap here shows up as egui and the renderer fighting over the
/// same strip of screen every frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkstationRects {
    pub top: Rect,
    pub left: Rect,
    pub right: Rect,
    pub bottom: Rect,
    pub status: Rect,
    /// What is left for the renderer. May be empty on an absurdly small window; callers
    /// must skip rendering rather than create a zero-sized texture.
    pub viewport: Rect,
}

/// The smallest viewport we will squeeze the docks for. Below this the docks start
/// yielding; below *that* the viewport is allowed to vanish rather than go negative.
pub const MIN_VIEWPORT: (f32, f32) = (320.0, 240.0);

/// Partition `window` into the workstation regions.
///
/// Order of yielding, when the window is too small for everything: the **horizontal**
/// docks (left/right) shrink together, then the **bottom** dock, then the top strip and
/// status bar. The viewport is the thing being protected — it is the subject of the
/// application, and a workstation whose viewport has been squeezed to nothing by its own
/// furniture is the failure mode this ordering exists to prevent.
pub fn layout_workstation(window: Rect, docks: DockSizes) -> WorkstationRects {
    let ww = window.w.max(0.0);
    let wh = window.h.max(0.0);

    // Vertical budget: top + bottom + status must leave MIN_VIEWPORT.1 for the viewport.
    let mut top = docks.top.max(0.0);
    let mut status = docks.status.max(0.0);
    let mut bottom = docks.bottom.max(0.0);
    let v_fixed = top + status;
    let v_avail = (wh - v_fixed - MIN_VIEWPORT.1).max(0.0);
    if bottom > v_avail {
        bottom = v_avail;
    }
    // Still over budget? The chrome itself must yield, proportionally.
    let v_used = top + status + bottom;
    if v_used > wh {
        let scale = if v_used > 0.0 { wh / v_used } else { 0.0 };
        top *= scale;
        status *= scale;
        bottom *= scale;
    }

    // Horizontal budget: left + right must leave MIN_VIEWPORT.0.
    let mut left = docks.left.max(0.0);
    let mut right = docks.right.max(0.0);
    let h_avail = (ww - MIN_VIEWPORT.0).max(0.0);
    if left + right > h_avail {
        let want = left + right;
        let scale = if want > 0.0 { h_avail / want } else { 0.0 };
        left *= scale;
        right *= scale;
    }
    // And if even that overflows the window (a window narrower than MIN_VIEWPORT), clamp.
    if left + right > ww {
        let want = left + right;
        let scale = if want > 0.0 { ww / want } else { 0.0 };
        left *= scale;
        right *= scale;
    }

    let top_r = Rect::new(window.x, window.y, ww, top);
    let status_r = Rect::new(window.x, window.y + wh - status, ww, status);
    let bottom_r = Rect::new(window.x, status_r.y - bottom, ww, bottom);

    // The side docks span from under the top strip to the top of the bottom dock.
    let mid_y = window.y + top;
    let mid_h = (bottom_r.y - mid_y).max(0.0);
    let left_r = Rect::new(window.x, mid_y, left, mid_h);
    let right_r = Rect::new(window.x + ww - right, mid_y, right, mid_h);

    let viewport = Rect::new(
        window.x + left,
        mid_y,
        (ww - left - right).max(0.0),
        mid_h,
    );

    WorkstationRects { top: top_r, left: left_r, right: right_r, bottom: bottom_r, status: status_r, viewport }
}

// ═════════════════════════════════════════════════════════════════════════════
// The egui shell (#532 Tier 1)
// ═════════════════════════════════════════════════════════════════════════════

/// The right rail's fixed width (`SidePanel::right("presets_panel")` in `lib.rs`).
pub const PRESETS_RAIL_W: f32 = 150.0;

/// Dock sizes to hand egui, in logical points.
///
/// Tier 1 draws the workstation inside the editor window rather than a window of its
/// own, so two of the five regions already exist as chrome: the right dock **is** the
/// presets rail, and the status bar **is** the perf strip. This returns the two that
/// are new, already squeezed by [`layout_workstation`]'s rule that the docks yield
/// before the middle does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EguiDocks {
    /// Width of the left "Model" dock. `0` means do not show it at all.
    pub left: f32,
    /// Height of the bottom "Live Telemetry" dock. `0` means do not show it.
    pub bottom: f32,
}

/// Below this the dock is more of an obstruction than a readout, so it is dropped
/// entirely rather than rendered as a useless sliver.
const DOCK_MIN_USEFUL: f32 = 96.0;

/// Height the bottom **Live Telemetry** dock asks for: what the (always-compact)
/// #482 dashboard actually needs to draw without being cut off.
///
/// The dock previously asked for [`DockSizes::default()`]`.bottom` — 220 pt, a
/// figure from the PRD mock rather than from the widgets — while the dashboard
/// drew appreciably more than that. egui clips a panel to its own rect
/// (`panel_ui.set_clip_rect(panel_rect)`), so the cards were simply **cut off at
/// the bottom of the window**, and were cut off *identically* when the window was
/// maximized: this height is absolute, so every pixel a bigger window added went
/// to the middle and none to the dock.
///
/// Derived from the layout rather than guessed — the tallest of the three columns
/// governs, which is column 1:
///
/// ```text
///   header 18 + description ~30 + spacing ~8            =  56
///   Head × layer card: 18 title + 14 provenance + 86 plot + 22 frame/pad = 140
///   Heartbeat card:    18 title + 14 provenance + 58 plot + 22 frame/pad = 112
///   panel frame margin                                  =  12
///                                                        -----
///                                                          320
/// ```
///
/// Keep this in step with the widget heights in `lib.rs::mind_dashboard_ui`. The
/// dock still yields to the squeeze rule below, and the dashboard still sits in a
/// `ScrollArea`, so being wrong here degrades to scrolling rather than to clipping.
///
/// ✏️ **+24 for #147 Tier 4's training strip, and only for its quiet case.** The strip is
/// one row above the three columns, and what it costs depends on what it has to say:
///
/// ```text
///   no UNSLOTH_API_KEY (the ordinary case)   one weak line + spacing  =  24
///   a configured Studio, nothing training    a card, ~84
///   a live run                               that card plus curves, ~150
/// ```
///
/// 📌 **Only the first is budgeted here, deliberately.** This constant is an *absolute*
/// height, so every point added is taken from the viewport on every machine forever —
/// including the great majority with no Unsloth Studio. The other two land in the
/// `ScrollArea`, which is the documented degradation one paragraph up, and it is the right
/// trade: you scroll to a training run while it is happening, you do not give it permanent
/// screen on a machine that never trains. `lib.rs::mind_training_ui` draws the no-key case
/// as a bare line rather than a card precisely so this number could stay small.
pub const DASHBOARD_H: f32 = 344.0;

/// Size the Tier 1 docks for a window of `window_w` × `window_h` points.
///
/// `status_h` is the perf strip's height (0 when it is closed), so the arithmetic sees
/// the same vertical budget the user does.
pub fn egui_docks(window_w: f32, window_h: f32, status_h: f32) -> EguiDocks {
    let docks = DockSizes {
        // The egui shell draws no top strip — the tab bar and title live inside the
        // middle. Reserving the mock's 44 pt for furniture that is never drawn just
        // took that budget away from the bottom dock.
        top: 0.0,
        right: PRESETS_RAIL_W,
        bottom: DASHBOARD_H,
        status: status_h,
        ..DockSizes::default()
    };
    let r = layout_workstation(Rect::new(0.0, 0.0, window_w, window_h), docks);
    EguiDocks {
        left: if r.left.w < DOCK_MIN_USEFUL { 0.0 } else { r.left.w },
        bottom: if r.bottom.h < DOCK_MIN_USEFUL { 0.0 } else { r.bottom.h },
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Input routing
// ═════════════════════════════════════════════════════════════════════════════

/// Who a pointer event belongs to this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerTarget {
    /// egui — a dock, a menu, a slider, or a pane header.
    Ui,
    /// The viewport camera (orbit / pan / zoom).
    Viewport,
    /// Neither wants it.
    Nothing,
}

/// What egui reported it wants this frame, plus where the pointer is.
#[derive(Clone, Copy, Debug)]
pub struct PointerContext {
    /// `egui::Context::wants_pointer_input()`.
    pub egui_wants_pointer: bool,
    /// Cursor position in logical points, `None` when outside the window.
    pub pointer: Option<(f32, f32)>,
    /// The current viewport rectangle from [`layout_workstation`].
    pub viewport: Rect,
}

/// Routes pointer events between egui and the viewport camera, **with capture**.
///
/// Capture is the whole point. Without it, an orbit drag that starts in the viewport and
/// wanders over the Properties dock hands the camera to egui mid-gesture and the view
/// snaps — which is exactly how these hybrid apps come to feel cheap. Once a drag is
/// claimed, it stays claimed until the button is released, regardless of what is under
/// the cursor or what egui would like.
#[derive(Default, Debug)]
pub struct PointerRouter {
    captured: Option<PointerTarget>,
}

impl PointerRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// True while a drag gesture is claimed by someone.
    pub fn is_capturing(&self) -> bool {
        self.captured.is_some()
    }

    /// Decide where a *button press* goes, and claim the gesture for that target.
    ///
    /// egui wins ties: if it says it wants the pointer (a slider is under the cursor, a
    /// combo box is open, a panel edge is being dragged), it gets the event even when the
    /// cursor is geometrically inside the viewport — egui's panels and popups can overlap
    /// the viewport rectangle, and its own hit-testing is the authority on that.
    pub fn on_press(&mut self, cx: PointerContext) -> PointerTarget {
        let target = Self::hover_target(cx);
        self.captured = Some(target);
        target
    }

    /// Where a *motion* event goes: the captured target if a gesture is in flight,
    /// otherwise plain hover routing.
    pub fn on_move(&self, cx: PointerContext) -> PointerTarget {
        match self.captured {
            Some(t) => t,
            None => Self::hover_target(cx),
        }
    }

    /// Release the gesture. Returns whoever had it, so the caller can end a camera drag.
    pub fn on_release(&mut self) -> PointerTarget {
        self.captured.take().unwrap_or(PointerTarget::Nothing)
    }

    /// Scroll follows the **captured** target when a drag is in flight (so a scroll
    /// mid-orbit still zooms the camera), and hover otherwise.
    pub fn on_scroll(&self, cx: PointerContext) -> PointerTarget {
        match self.captured {
            Some(t) => t,
            None => Self::hover_target(cx),
        }
    }

    fn hover_target(cx: PointerContext) -> PointerTarget {
        if cx.egui_wants_pointer {
            return PointerTarget::Ui;
        }
        match cx.pointer {
            Some((x, y)) if cx.viewport.contains(x, y) => PointerTarget::Viewport,
            _ => PointerTarget::Nothing,
        }
    }
}

/// Keyboard routing is simpler: there is no capture, because a text field either has
/// focus or it does not. When egui wants keys (a rename field, the future command
/// palette), the viewport's shortcuts (`F`, `H`, `O`, …) must not fire — otherwise
/// typing "F" into a session name toggles fullscreen.
pub fn route_keyboard(egui_wants_keyboard: bool) -> PointerTarget {
    if egui_wants_keyboard {
        PointerTarget::Ui
    } else {
        PointerTarget::Viewport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── layout ──────────────────────────────────────────────────────────────

    fn win(w: f32, h: f32) -> Rect {
        Rect::new(0.0, 0.0, w, h)
    }

    /// The regions must exactly tile the window: a gap leaves an unpainted strip, an
    /// overlap makes egui and the renderer fight over the same pixels every frame.
    #[test]
    fn regions_tile_the_window_exactly() {
        let w = win(1680.0, 940.0);
        let r = layout_workstation(w, DockSizes::default());
        let sum = r.top.area()
            + r.left.area()
            + r.right.area()
            + r.bottom.area()
            + r.status.area()
            + r.viewport.area();
        assert!(
            (sum - w.area()).abs() < 0.01,
            "regions must tile exactly: {sum} vs {}",
            w.area()
        );
        // Vertical stack is contiguous and in order.
        assert_eq!(r.top.y + r.top.h, r.viewport.y);
        assert_eq!(r.viewport.y + r.viewport.h, r.bottom.y);
        assert_eq!(r.bottom.y + r.bottom.h, r.status.y);
        assert_eq!(r.status.y + r.status.h, w.h);
        // Horizontal band is contiguous and in order.
        assert_eq!(r.left.x + r.left.w, r.viewport.x);
        assert_eq!(r.viewport.x + r.viewport.w, r.right.x);
        assert_eq!(r.right.x + r.right.w, w.w);
    }

    #[test]
    fn side_docks_share_the_viewport_band() {
        let r = layout_workstation(win(1680.0, 940.0), DockSizes::default());
        assert_eq!(r.left.y, r.viewport.y);
        assert_eq!(r.left.h, r.viewport.h);
        assert_eq!(r.right.y, r.viewport.y);
        assert_eq!(r.right.h, r.viewport.h);
    }

    /// The docks yield before the viewport does. This is the ordering that keeps the
    /// window usable on a laptop, which is the complaint #520 Tier 1 surfaced.
    #[test]
    fn docks_yield_to_protect_the_viewport() {
        let d = DockSizes::default();
        // A window narrower than left+right+MIN_VIEWPORT.0.
        let r = layout_workstation(win(700.0, 940.0), d);
        assert!(
            r.viewport.w >= MIN_VIEWPORT.0 - 0.01,
            "viewport {} should be protected to {}",
            r.viewport.w,
            MIN_VIEWPORT.0
        );
        assert!(r.left.w < d.left, "left dock should have yielded");
        assert!(r.right.w < d.right, "right dock should have yielded");
        // And they yield *proportionally* — the wider dock stays the wider one.
        assert!(r.right.w > r.left.w);
    }

    #[test]
    fn bottom_dock_yields_before_the_viewport_vertically() {
        let d = DockSizes::default();
        let r = layout_workstation(win(1680.0, 400.0), d);
        assert!(r.bottom.h < d.bottom, "bottom dock should have yielded");
        assert!(r.viewport.h >= MIN_VIEWPORT.1 - 0.01);
    }

    /// A window smaller than the minimum viewport must still produce a sane, non-negative
    /// partition — the viewport is allowed to vanish, but nothing may go negative or NaN.
    #[test]
    fn absurdly_small_windows_stay_sane() {
        for (w, h) in [(0.0, 0.0), (10.0, 10.0), (200.0, 120.0), (1.0, 900.0)] {
            let r = layout_workstation(win(w, h), DockSizes::default());
            for (name, rect) in [
                ("top", r.top),
                ("left", r.left),
                ("right", r.right),
                ("bottom", r.bottom),
                ("status", r.status),
                ("viewport", r.viewport),
            ] {
                assert!(rect.w >= 0.0 && rect.h >= 0.0, "{name} went negative at {w}×{h}");
                assert!(
                    rect.w.is_finite() && rect.h.is_finite() && rect.x.is_finite() && rect.y.is_finite(),
                    "{name} went non-finite at {w}×{h}"
                );
            }
            let sum = r.top.area() + r.left.area() + r.right.area() + r.bottom.area() + r.status.area() + r.viewport.area();
            assert!((sum - (w.max(0.0) * h.max(0.0))).abs() < 0.01, "tiling broke at {w}×{h}");
        }
    }

    #[test]
    fn zero_docks_give_the_whole_window_to_the_viewport() {
        let none = DockSizes { top: 0.0, left: 0.0, right: 0.0, bottom: 0.0, status: 0.0 };
        let r = layout_workstation(win(1280.0, 860.0), none);
        assert_eq!(r.viewport, Rect::new(0.0, 0.0, 1280.0, 860.0));
    }

    // ── pointer routing ─────────────────────────────────────────────────────

    fn cx(wants: bool, at: Option<(f32, f32)>) -> PointerContext {
        PointerContext {
            egui_wants_pointer: wants,
            pointer: at,
            viewport: Rect::new(260.0, 44.0, 1120.0, 650.0),
        }
    }

    #[test]
    fn hover_goes_to_the_viewport_only_when_egui_does_not_want_it() {
        let r = PointerRouter::new();
        assert_eq!(r.on_move(cx(false, Some((600.0, 300.0)))), PointerTarget::Viewport);
        // egui wins ties even inside the viewport rect — its popups overlap it.
        assert_eq!(r.on_move(cx(true, Some((600.0, 300.0)))), PointerTarget::Ui);
        // Outside the viewport and unwanted by egui: nobody.
        assert_eq!(r.on_move(cx(false, Some((10.0, 300.0)))), PointerTarget::Nothing);
        assert_eq!(r.on_move(cx(false, None)), PointerTarget::Nothing);
    }

    /// **The test this router exists for.** A camera drag that starts in the viewport
    /// keeps the camera even when the cursor wanders over a dock and egui starts asking
    /// for the pointer. Without capture the view snaps mid-orbit.
    #[test]
    fn a_drag_started_in_the_viewport_is_not_stolen_by_egui() {
        let mut r = PointerRouter::new();
        assert_eq!(r.on_press(cx(false, Some((600.0, 300.0)))), PointerTarget::Viewport);
        // Cursor leaves the viewport and egui now wants the pointer — irrelevant.
        assert_eq!(r.on_move(cx(true, Some((100.0, 300.0)))), PointerTarget::Viewport);
        assert_eq!(r.on_move(cx(true, None)), PointerTarget::Viewport);
        assert_eq!(r.on_scroll(cx(true, Some((100.0, 300.0)))), PointerTarget::Viewport);
        assert!(r.is_capturing());
        assert_eq!(r.on_release(), PointerTarget::Viewport);
        assert!(!r.is_capturing());
        // After release, normal hover routing resumes.
        assert_eq!(r.on_move(cx(true, Some((100.0, 300.0)))), PointerTarget::Ui);
    }

    /// The mirror case: a slider drag that wanders into the viewport must not start
    /// orbiting the camera.
    #[test]
    fn a_drag_started_in_the_ui_never_reaches_the_camera() {
        let mut r = PointerRouter::new();
        assert_eq!(r.on_press(cx(true, Some((100.0, 300.0)))), PointerTarget::Ui);
        assert_eq!(r.on_move(cx(false, Some((600.0, 300.0)))), PointerTarget::Ui);
        assert_eq!(r.on_scroll(cx(false, Some((600.0, 300.0)))), PointerTarget::Ui);
        assert_eq!(r.on_release(), PointerTarget::Ui);
    }

    #[test]
    fn release_without_press_is_harmless() {
        let mut r = PointerRouter::new();
        assert_eq!(r.on_release(), PointerTarget::Nothing);
        assert!(!r.is_capturing());
    }

    /// Typing into a text field must not fire the viewport's single-key shortcuts.
    #[test]
    fn keyboard_follows_egui_focus() {
        assert_eq!(route_keyboard(true), PointerTarget::Ui);
        assert_eq!(route_keyboard(false), PointerTarget::Viewport);
    }

    // ── the egui shell (#532 Tier 1) ────────────────────────────────────────

    /// At a normal window size both docks are their full requested size — the point of
    /// the squeeze rule is that it does nothing until the window is actually small.
    #[test]
    fn docks_are_full_size_on_a_roomy_window() {
        let d = egui_docks(1680.0, 940.0, 26.0);
        assert_eq!(d.left, DockSizes::default().left);
        assert_eq!(d.bottom, DASHBOARD_H);
    }

    /// The bottom dock must be able to hold the dashboard it exists to show. This is
    /// the regression the 220 pt mock figure caused: the panel clips to its own rect,
    /// so a dock shorter than its content silently cuts the cards off instead of
    /// reporting anything. Any window tall enough to have the dock at all must give it
    /// the full height.
    #[test]
    fn the_bottom_dock_fits_the_dashboard_on_a_normal_window() {
        for (w, h) in [(1280.0, 860.0), (1680.0, 940.0), (2560.0, 1440.0)] {
            let d = egui_docks(w, h, 0.0);
            assert_eq!(
                d.bottom, DASHBOARD_H,
                "dashboard would be clipped at {w}x{h} (dock {} < {DASHBOARD_H})",
                d.bottom
            );
        }
    }

    /// The docks must never claim more of the window than exists, at any size. This is
    /// the property that keeps egui from being handed a negative central rect.
    #[test]
    fn docks_never_exceed_the_window() {
        let mut w = 40.0f32;
        while w < 2600.0 {
            let mut h = 40.0f32;
            while h < 1600.0 {
                let d = egui_docks(w, h, 26.0);
                // The left dock must never claim width the fixed presets rail already
                // owns. (On a window narrower than the rail itself the rail overflows
                // on its own — that is the rail's business, not this function's, and
                // the bound below is 0 there, which is exactly what we return.)
                assert!(
                    d.left <= (w - PRESETS_RAIL_W).max(0.0) + 0.01,
                    "left dock {} does not leave room for the rail at width {w}",
                    d.left
                );
                assert!(d.bottom <= h + 0.01, "bottom dock {} overflows height {h}", d.bottom);
                assert!(d.left >= 0.0 && d.bottom >= 0.0);
                h += 37.0;
            }
            w += 53.0;
        }
    }

    /// Shrinking the window shrinks the furniture, not the middle: the docks are
    /// monotonically non-increasing as the window gets smaller.
    #[test]
    fn docks_yield_monotonically_as_the_window_shrinks() {
        let mut prev = egui_docks(2400.0, 1400.0, 26.0);
        for w in (400..2400).rev().step_by(97) {
            let d = egui_docks(w as f32, 1400.0, 26.0);
            assert!(d.left <= prev.left + 0.01, "left grew as the window shrank at w={w}");
            prev = d;
        }
    }

    /// A dock too small to read is dropped rather than rendered as a sliver, so a tiny
    /// window shows the controls instead of three useless stripes.
    #[test]
    fn a_useless_sliver_is_dropped_entirely() {
        let d = egui_docks(360.0, 300.0, 26.0);
        assert_eq!(d.left, 0.0, "a sliver-width dock should be dropped");
        assert_eq!(d.bottom, 0.0, "a sliver-height dock should be dropped");
    }
}

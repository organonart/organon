//! **The viewport's camera input** (#621) — how a drag inside an editor pane reaches `World`.
//!
//! # The gap this closes
//!
//! #593 gave Organon Mind a real GPU viewport and #617 Tier 1 gave it two shapes
//! (*workstation*, a bounded pane egui lays out; *immersive*, the whole window). Both were
//! **look-only**: `wgpu_editor`'s `WindowHandler::on_event` handed every event to egui and never
//! touched `World`, so there was a native-resolution viewport with no way to move the camera in
//! it. This module is the missing hop.
//!
//! # Option A, and why
//!
//! #614 left `World::on_window_event` `winit::event::WindowEvent`-typed on purpose: it is the
//! winit host's entry point, it carries the visual's whole keymap (**H**, **U**, **Esc**,
//! **R**, …), and a baseview host never calls it. #621 forces the choice that note deferred —
//! widen that entry point until the whole keymap crosses (**B**), or give the world a second,
//! backend-neutral one for the camera alone (**A**).
//!
//! **A.** [`CameraInput`] is two gestures — orbit and zoom — and `World::apply_camera_input` is
//! the one place either is implemented; the winit arms in `on_window_event` now *delegate* to
//! it, so the visual and the editor cannot drift apart. The keymap does not cross, because most
//! of it is a **projector** concern that means nothing in a docked pane (**F** fullscreen, **R**
//! / **B** / **C** / **V** recording, **O** environment picker) and one key of it — **Esc**,
//! which quits — has an unresolved owner inside a plugin editor. A viewport you cannot orbit is
//! not a viewport; a viewport without **Shift+B** is fine. Widening the seam to carry keys is
//! still available later and nothing here forecloses it.
//!
//! # ⚠️ Why the events do **not** come through `PointerRouter`
//!
//! `mind_shell::PointerRouter` is the obvious source — it is written, tested, winit-free, and
//! `UiLayer::on_window_event` already runs it. **It cannot work in this host**, and the reason is
//! mechanical rather than a matter of taste:
//!
//! ```text
//! egui: Context::is_pointer_over_area()
//!   → if the layer under the pointer is Order::Background:
//!         !pass_state.unused_rect.contains(pointer)
//! egui: PassState::allocate_central_panel()
//!   → self.unused_rect = Rect::NOTHING;   // "Nothing left unused after this"
//! ```
//!
//! `editor_ui` draws a `CentralPanel` (that is what `ResizableWindow` is), so `unused_rect` is
//! `NOTHING` for the whole frame and **`Context::wants_pointer_input()` is unconditionally true
//! everywhere in the window** — which is the router's `egui_wants_pointer`, and egui wins ties.
//! Every event would route to `PointerTarget::Ui` and the camera would get nothing. The test
//! `a_central_panel_makes_egui_want_every_pointer` measures exactly that rather than asserting it.
//!
//! The visual's own window escapes this only because it draws `ui_layer::hud` as a floating
//! `egui::Window` and *no* central panel — so `unused_rect` stays the content rect and the router
//! is answering a real question there. Nothing about the router is wrong; it is answering a
//! question this host cannot pose.
//!
//! So the authority here is **egui's widget hit-test**, which is the thing that actually knows
//! where the interface is once a central panel is in play: [`scene_viewport`] registers the scene
//! as a drag-sensing widget and reads its `Response`. That buys three properties the router would
//! have had to reproduce, and one it could not:
//!
//! - **capture** — `Response::dragged()` stays true for the whole gesture wherever the pointer
//!   wanders, which is precisely why `PointerRouter` exists;
//! - **arbitration** — egui's `hit_test` already prefers "a smaller thing on a big drag
//!   background" (its words), so a slider over the scene keeps its own drag;
//! - **no fight with the `ScrollArea`** — in workstation mode the pane registers *after* the
//!   scroll area, and egui breaks a tie by taking the topmost;
//! - **coordinates that cannot go stale.** `Response::drag_delta()` is
//!   `input.pointer.delta()` — the pointer's motion in **screen** space — so it is unaffected by
//!   where the pane currently sits. That is the whole workstation trap (a pane offset by its rect
//!   *and* by the scroll offset) dissolved rather than solved: there is no mapping to get wrong,
//!   and [`orbit_pixels`] is the only conversion left.
//!
//! # Units
//!
//! | | the visual (winit) | the editor (egui) | this module |
//! |---|---|---|---|
//! | orbit | `CursorMoved` delta, **physical px** | `drag_delta()`, **points** | [`orbit_pixels`] multiplies by `pixels_per_point` so the two rigs orbit at the same rate on the same display |
//! | zoom | `LineDelta(_, y) * 20.0` / `PixelDelta(p).y` | `smooth_scroll_delta.y`, **points** | passed through **unscaled** — see [`SceneGesture::zoom`] |
//!
//! No `Shared` / IPC / `LAYOUT_VERSION` change: everything here is editor-side state that dies
//! with the window. The mode and the camera stay off the preset system for the same reason #617
//! kept `immersive` on `PresetUi` — recalling a Scene must not move the camera.

/// The stable `egui::Id` the scene region is registered under.
///
/// One id, because the two modes are exclusive: workstation registers the pane's rect and
/// immersive registers the window's, never both in a frame.
pub const SCENE_VIEWPORT_ID: &str = "organon-scene-viewport";

/// One camera gesture, in the units `World::on_window_event`'s winit arms have always used.
///
/// This is the whole backend-neutral entry point — deliberately two variants and not a widened
/// event enum. See the module docs on Option A.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CameraInput {
    /// Pointer motion while the scene owns the drag, in **physical pixels**.
    ///
    /// The world consumes it as either an orbit (`yaw`/`pitch`) or, while riding the #187 rails,
    /// a bore-clamped lean — which is why this is "the drag" rather than "the orbit".
    Orbit { dx: f32, dy: f32 },
    /// Wheel motion, in the unit the visual's `MouseScrollDelta` arm produces. Positive = toward
    /// the subject.
    Zoom { dy: f32 },
}

/// What one editor frame's pointer interaction asked the camera for.
///
/// Accumulated by [`scene_viewport`] while `editor_ui` runs and drained by the host after it, so
/// a gesture reaches the camera in the frame it was made. Transient editor state: **never** in
/// `Shared`, never in a preset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneGesture {
    /// Accumulated pointer motion in **physical pixels** — already through [`orbit_pixels`].
    pub orbit: (f32, f32),
    /// Accumulated wheel motion in **egui points**, passed to the world unscaled.
    ///
    /// ⚠️ **Deliberately not multiplied by `pixels_per_point`.** egui turns one wheel notch into
    /// `line_scroll_speed` = 40 points regardless of display scale, so leaving it alone makes the
    /// zoom rate a property of the wheel rather than of the monitor. Scaling it would make one
    /// notch worth twice as much on a Retina display as on the projector beside it, which is the
    /// kind of difference nobody attributes to the right cause. The visual's own line arm is
    /// scale-independent too (`y * 20.0`), so this matches it in *kind*; a notch here is a little
    /// over twice as far, which is the wheel actually being usable.
    pub zoom: f32,
}

impl SceneGesture {
    /// Nothing asked for.
    pub const NONE: Self = Self { orbit: (0.0, 0.0), zoom: 0.0 };

    /// Whether there is anything for the world to do.
    pub fn is_empty(&self) -> bool {
        *self == Self::NONE
    }

    /// Take the gesture and leave nothing behind. The host calls this once per frame; a gesture
    /// that is not drained must not be applied twice.
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::NONE)
    }

    /// The gesture as the camera inputs it implies, in the order they should be applied.
    ///
    /// Empty components are skipped rather than sent as zero, so a frame with no interaction
    /// costs the world nothing at all.
    pub fn inputs(&self) -> impl Iterator<Item = CameraInput> {
        let orbit = (self.orbit != (0.0, 0.0))
            .then_some(CameraInput::Orbit { dx: self.orbit.0, dy: self.orbit.1 });
        let zoom = (self.zoom != 0.0).then_some(CameraInput::Zoom { dy: self.zoom });
        orbit.into_iter().chain(zoom)
    }
}

/// Which shape the scene has in the window this frame (#617 Tier 1).
///
/// The two are **not the same input problem**, which is the whole reason this is an enum rather
/// than a bool tucked into a call: workstation's region is a bounded pane that egui laid out and
/// that competes with the widgets around it, immersive's is the whole window sitting *under* the
/// entire interface. See [`press_belongs_to_the_scene`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneMode {
    /// The scene is a pane inside the workstation — a widget among widgets.
    Workstation,
    /// The scene is the window and the interface floats over it.
    Immersive,
}

impl SceneMode {
    /// The mode `PresetUi::immersive` selects.
    pub fn from_immersive(immersive: bool) -> Self {
        if immersive {
            Self::Immersive
        } else {
            Self::Workstation
        }
    }
}

/// The editor-side scene-input state: this frame's gesture plus the in-flight drag's owner.
///
/// Lives on `PresetUi` beside `immersive` and `scene_pane_rect`, which is where #617 put view
/// state that is neither a parameter nor a preset field.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneInput {
    /// What this frame asked for. Drained by the host; see [`SceneGesture::take`].
    pub gesture: SceneGesture,
    /// Whether the drag currently in flight is the scene's.
    ///
    /// Latched at the press and held for the whole gesture, so the answer cannot change halfway
    /// through — the property `PointerRouter` calls capture.
    claimed: bool,
}

impl SceneInput {
    /// Whether a scene drag is in flight. Exposed for the host, and for tests.
    pub fn is_dragging(&self) -> bool {
        self.claimed
    }
}

/// Convert a drag delta egui reported in **points** into the **physical pixels** the world's
/// camera has always been driven in.
///
/// The visual's `CursorMoved` arm reads `PhysicalPosition`, so on a Retina display a 100-point
/// drag turns the camera twice as far as it does at 1×. That is a pre-existing property of the
/// rig, not a bug to fix behind James's back — and the editor and the visual sitting on the same
/// desk had better agree. So the editor reproduces it rather than quietly normalising.
///
/// A scale that cannot be multiplied by (a backend has been seen to report `0`, negative and NaN
/// while a window is moving between displays) falls back to `1.0`, matching
/// `WindowGeometry::scale`.
pub fn orbit_pixels(drag_delta_points: (f32, f32), pixels_per_point: f32) -> (f32, f32) {
    let s = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { pixels_per_point } else { 1.0 };
    (drag_delta_points.0 * s, drag_delta_points.1 * s)
}

/// The pane's size in **physical pixels**, given its size in points and the display scale.
///
/// Hoisted out of `wgpu_editor::render_scene_pane` so the clamps are a test rather than a
/// comment. Both bounds are load-bearing: egui can hand back a zero or negative rect for one
/// frame while a layout settles, and a zero-sized texture is a wgpu validation error rather than
/// a blank pane; the upper bound is a rail against a runaway layout asking for a texture no
/// device will make.
pub fn pane_pixels(size_points: (f32, f32), scale: f32) -> (u32, u32) {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let px = |v: f32| {
        let v = v * s;
        if v.is_finite() { (v.round().max(1.0) as u32).clamp(16, 8192) } else { 16 }
    };
    (px(size_points.0), px(size_points.1))
}

/// The pane's size in **physical pixels**, taken as its *fraction of the window* rather than
/// by multiplying its points by a reported scale.
///
/// # Why a scale cannot be trusted here, and a ratio can
///
/// [`pane_pixels`] believes the scale it is handed, which is right for a caller holding a
/// scale it just measured. A caller that has to *remember* one is in a different position:
/// egui reports `pixels_per_point` as a frame **output**, so the first frame's texture must be
/// sized before any scale exists, and whatever stands in for "not measured yet" gets
/// multiplied by a real pane. Spelled `1.0` — the reading that looks harmless — it is
/// indistinguishable from a genuine 100 % display, so the pane is sized in **points**: on
/// ORGANON-ONE's 225 % display, 1100×690 where 2475×1553 was meant, 2.25× too small in each
/// axis.
///
/// A live render target survives that: it is rebuilt the moment the real scale lands. What
/// does not survive is anything **copied** from it in the meantime. Console Spike Tier 4's
/// epoch cache copies the live backdrop when a look closes and never re-renders it
/// (`shell_main.rs`'s `snapshot_live_backdrop`), so a snapshot taken inside that window keeps
/// the small picture for the rest of the session and every band painted from it is magnified
/// back up — measured at exactly 2.25×, the older epochs blurred while the live band stayed
/// crisp.
///
/// The swapchain has no equivalent hazard: it is configured from `Window::inner_size`, which
/// is physical *by definition* and correct before any scale is known. So this takes the ratio
/// instead — pane points over window points, both read from the same egui frame — and applies
/// it to the swapchain. Any error in `pixels_per_point` scales numerator and denominator
/// alike and **cancels exactly**, which makes a point-resolution answer unrepresentable rather
/// than merely unlikely.
///
/// A ratio that cannot be computed (a window egui reports as zero, negative or NaN while a
/// layout settles) falls back to the whole swapchain — the same "one frame of the window
/// instead of the pane" the first frame already had, and never a point-sized texture. The
/// clamps are [`pane_pixels`]'s, for [`pane_pixels`]'s reasons.
pub fn pane_pixels_in(
    swapchain: (u32, u32),
    pane_points: (f32, f32),
    window_points: (f32, f32),
) -> (u32, u32) {
    let px = |pane: f32, window: f32, chain: u32| {
        let usable = pane.is_finite() && pane > 0.0 && window.is_finite() && window > 0.0;
        let frac = if usable { (pane / window).min(1.0) } else { 1.0 };
        let v = chain as f32 * frac;
        if v.is_finite() { (v.round().max(1.0) as u32).clamp(16, 8192) } else { 16 }
    };
    (
        px(pane_points.0, window_points.0, swapchain.0),
        px(pane_points.1, window_points.1, swapchain.1),
    )
}

/// Does a press egui just handed the scene region really belong to the scene?
///
/// **The two modes need different answers, and this is where that lives.**
///
/// - **Workstation** — the region *is* the pane, so egui only hands it a drag when the pane won
///   the hit-test against everything laid out around it. There is nothing left to second-guess:
///   `true`.
/// - **Immersive** — the region is the whole window and is registered *before* the interface, so
///   egui hands it every drag the interface did not claim **as a drag**. A press that began on a
///   *button* is not a drag as far as egui is concerned — its hit-test reports
///   `click: the button, drag: the background` — so it arrives here anyway and has to be given
///   back. `interactive_under_pointer` is that count.
///
/// ⚠️ **"Interactive" means click- or drag-sensing, and the distinction is load-bearing.**
/// Counting *every* widget under the pointer does not work and was tried: egui registers a
/// `WidgetRect` for **every `Ui`**, including the central panel's and the scroll area's, so a
/// plain count is never zero anywhere in the window and the immersive camera would never move.
/// Filtering to the ones that actually sense a gesture is what makes the number mean "the
/// interface has a control here".
///
/// The consequence, stated rather than discovered later: a press on a card's **inert** face — a
/// label, a heading, the frame's padding — is *not* a control, so it orbits. In immersive mode
/// that is the intent rather than a leak. The whole premise of the mode is that the interface
/// floats over a world that is showing through everywhere it has not put a control, and a
/// drag-anywhere-you-can-see-it camera is what makes it an instrument rather than a screenshot.
/// Workstation mode, where the scene is bounded and the cards are the subject, never asks.
pub fn press_belongs_to_the_scene(mode: SceneMode, interactive_under_pointer: usize) -> bool {
    match mode {
        SceneMode::Workstation => true,
        SceneMode::Immersive => interactive_under_pointer == 0,
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// The egui side
// ═════════════════════════════════════════════════════════════════════════════

/// Register `rect` as the scene's interaction region and accumulate whatever it was asked for.
///
/// Call it **once per frame**, from `editor_ui`, at the place the mode dictates:
///
/// - **workstation** — at the pane, with the rect `allocate_exact_size` just reserved, so the
///   region is registered *after* the surrounding scroll area and wins a tie against its
///   drag-to-scroll background;
/// - **immersive** — at the top of the central panel with `ui.max_rect()`, *before* the
///   interface, so every widget drawn afterwards beats it in egui's hit-test.
///
/// Returns the region's `Response` so the caller can paint against the same rect.
pub fn scene_viewport(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    mode: SceneMode,
    st: &mut SceneInput,
) -> egui::Response {
    let id = egui::Id::new(SCENE_VIEWPORT_ID);
    // `Sense::drag()` and **not** `click_and_drag()`. A drag-only widget is what egui's hit-test
    // treats as "a big background thing", which is what lets it hand a click on top of us to the
    // button that wants it (`hit_test_on_close`'s `(Some(click), Some(drag))` arm). Sensing
    // clicks here would make the region the topmost click target and swallow the interface.
    let resp = ui.interact(rect, id, egui::Sense::drag());
    let ctx = ui.ctx().clone();
    // Workstation answers `true` without looking, so the hit-test walk below is immersive's cost
    // alone — and it is paid at most twice a frame, only while the pointer is in the window.
    let scene_is_free = || {
        press_belongs_to_the_scene(mode, interactive_under_pointer(&ctx, id))
    };

    if resp.drag_started() {
        st.claimed = scene_is_free();
    }
    if st.claimed && resp.dragged() {
        let d = resp.drag_delta();
        let (dx, dy) = orbit_pixels((d.x, d.y), ctx.pixels_per_point());
        st.gesture.orbit.0 += dx;
        st.gesture.orbit.1 += dy;
    }
    // **Keyed off "egui no longer reports a drag", not off `drag_stopped()`.** The latch is then
    // exactly as long-lived as egui's own drag, which is the one authority on when a gesture is
    // over — whereas `drag_stopped()` is a single-frame edge, and a frame that misses it (a mode
    // switch, a repaint the region was not registered on) would strand the latch on and leave the
    // scene claiming the wheel for the life of the window.
    if !resp.dragged() {
        st.claimed = false;
    }

    // ── zoom ────────────────────────────────────────────────────────────────
    // Hover, not capture-only: a wheel with no button down is the ordinary way to zoom. During a
    // claimed drag the scene keeps the wheel wherever the pointer has wandered to, which is the
    // same capture rule the orbit follows.
    let scene_owns_wheel = st.claimed || (resp.contains_pointer() && scene_is_free());
    if scene_owns_wheel {
        // **Consume it.** `ScrollArea` reads `smooth_scroll_delta` in its `end()`, i.e. *after*
        // its content — so zeroing it here, from inside, is what stops the workstation scrolling
        // out from under a zoom. Taking the smoothed value rather than the raw one is deliberate:
        // egui spreads one notch over several frames, and the total is preserved.
        let dy = ctx.input_mut(|i| {
            let dy = i.smooth_scroll_delta.y;
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            i.raw_scroll_delta = egui::Vec2::ZERO;
            dy
        });
        st.gesture.zoom += dy;
    }

    resp
}

/// How many **interactive** egui widgets other than the scene region contain the pointer.
///
/// egui's own hit-test result, filtered to widgets that sense a click or a drag — see
/// [`press_belongs_to_the_scene`] on why the filter is the whole point and an unfiltered count
/// is useless.
///
/// The two halves are read in sequence and not nested: `interaction_snapshot` and
/// `read_response` both take the context's lock, so asking the second question inside the
/// first's closure deadlocks. `read_response` is the documented way to ask about a widget
/// *before* it has been created this pass, which is exactly the position immersive's region is
/// in — it is registered first, and everything it is asking about comes later.
fn interactive_under_pointer(ctx: &egui::Context, own: egui::Id) -> usize {
    let under: Vec<egui::Id> = ctx.interaction_snapshot(|s| {
        s.contains_pointer.iter().copied().filter(|id| *id != own).collect()
    });
    under
        .into_iter()
        .filter(|id| {
            ctx.read_response(*id)
                .is_some_and(|r| r.sense.senses_click() || r.sense.senses_drag())
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═════════════════════════════════════════════════════════════════════════
    // The pure predicates
    // ═════════════════════════════════════════════════════════════════════════

    /// The editor and the visual must orbit at the same rate on the same display, which means
    /// points → physical pixels. Getting this wrong is the "feels subtly off" failure: it still
    /// orbits, just at half speed on every Retina machine.
    #[test]
    fn a_drag_orbits_in_physical_pixels() {
        assert_eq!(orbit_pixels((10.0, -4.0), 2.0), (20.0, -8.0));
        assert_eq!(orbit_pixels((10.0, -4.0), 1.0), (10.0, -4.0));
    }

    /// A scale that cannot be multiplied by must not send the camera to infinity. Backends report
    /// all three of these while a window is being created or dragged between displays.
    #[test]
    fn an_undividable_scale_orbits_at_one_to_one() {
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            assert_eq!(orbit_pixels((7.0, 3.0), bad), (7.0, 3.0), "scale {bad} must not survive");
        }
    }

    #[test]
    fn the_pane_is_sized_in_physical_pixels() {
        assert_eq!(pane_pixels((640.0, 360.0), 2.0), (1280, 720));
        assert_eq!(pane_pixels((640.0, 360.0), 1.0), (640, 360));
    }

    /// **The pane is never sized in points, whatever scale the frame was measured in.**
    ///
    /// The regression this pins is Console Spike Tier 4's band blur, reproduced on ORGANON-ONE
    /// (225 % display, `pixels_per_point` 2.25, a 1100×720-point window over a 30-point tab
    /// strip). Sizing the backdrop as `points × remembered_scale` sizes it as
    /// `points × 1.0` for as long as the scale is still the value that stood in for "not
    /// measured yet" — `1100×690` instead of `2475×1553`, 2.25× too small in each axis. The
    /// live target rebuilds itself the moment the real scale lands; the epoch snapshot copied
    /// from it does not, so the scrollback's older bands are magnified 2.25× forever.
    ///
    /// [`pane_pixels_in`] takes the pane's *fraction* of the window instead, so the scale
    /// cancels — which is what the sweep asserts: the identical window described in 1× points,
    /// in 2.25× points, or in any other unit yields the same physical texture.
    #[test]
    fn the_pane_is_sized_from_the_window_never_in_points() {
        // ORGANON-ONE's main display, measured: swapchain 2475×1620, pane 1100×690 points.
        let swapchain = (2475u32, 1620u32);
        let (pane, window) = ((1100.0, 690.0), (1100.0, 720.0));
        assert_eq!(pane_pixels_in(swapchain, pane, window), (2475, 1553));

        // The defect, spelled out: multiplying points by an unmeasured scale. Both of these
        // are what the console actually built, and the second is the bug.
        assert_eq!(pane_pixels(pane, 2.25), (2475, 1553), "the measured scale agrees");
        assert_eq!(pane_pixels(pane, 1.0), (1100, 690), "…and the unmeasured one does not");
        assert_ne!(
            pane_pixels_in(swapchain, pane, window),
            pane_pixels(pane, 1.0),
            "a point-sized backdrop must be unreachable from a window that is 2475px wide"
        );

        // The property that makes it unreachable: points are only ever compared with points,
        // so re-describing the same window in any unit changes nothing.
        for k in [0.25f32, 0.5, 1.0, 1.25, 2.0, 2.25, 3.0, 4.0] {
            assert_eq!(
                pane_pixels_in(
                    swapchain,
                    (pane.0 * k, pane.1 * k),
                    (window.0 * k, window.1 * k)
                ),
                (2475, 1553),
                "a frame whose points are {k}× must still size the pane in pixels"
            );
        }
    }

    /// A window egui reports as degenerate yields the whole swapchain — one frame of the
    /// window instead of the pane, which is what the first frame already showed — and never a
    /// texture sized in points.
    #[test]
    fn an_unusable_window_falls_back_to_the_swapchain_not_to_points() {
        let swapchain = (2475u32, 1620u32);
        for bad in [(0.0, 0.0), (-1100.0, 720.0), (f32::NAN, 720.0), (f32::INFINITY, 720.0)] {
            let got = pane_pixels_in(swapchain, (1100.0, 690.0), bad);
            assert_eq!(got.0, 2475, "window {bad:?} → {got:?}");
            assert!(got.1 == 1620 || got.1 == 1553, "window {bad:?} → {got:?}");
        }
        // …and a pane egui has not laid out yet is the same story, never a 16-pixel texture
        // stretched across the screen.
        assert_eq!(pane_pixels_in(swapchain, (0.0, 0.0), (1100.0, 720.0)), swapchain);
    }

    /// egui hands back a degenerate rect for a frame while a layout settles. A zero-sized texture
    /// is a validation error, not a blank pane, so the floor is not optional.
    #[test]
    fn a_degenerate_pane_is_clamped_rather_than_passed_on() {
        for bad in [(0.0, 0.0), (-40.0, 10.0), (f32::NAN, 100.0), (1e9, 1e9)] {
            let (w, h) = pane_pixels(bad, 2.0);
            assert!((16..=8192).contains(&w) && (16..=8192).contains(&h), "{bad:?} → {w}×{h}");
        }
    }

    /// Workstation trusts egui outright — the region *is* the pane, so a drag reaching it already
    /// beat everything laid out around it.
    #[test]
    fn workstation_takes_every_drag_egui_gives_the_pane() {
        assert!(press_belongs_to_the_scene(SceneMode::Workstation, 0));
        assert!(press_belongs_to_the_scene(SceneMode::Workstation, 3));
    }

    /// Immersive does not: its region is under the *whole* interface, so a press with a control
    /// beneath the pointer is the interface's, however far the region extends.
    #[test]
    fn immersive_gives_a_press_on_a_control_back_to_the_interface() {
        assert!(press_belongs_to_the_scene(SceneMode::Immersive, 0));
        assert!(!press_belongs_to_the_scene(SceneMode::Immersive, 1));
    }

    #[test]
    fn the_mode_follows_the_immersive_flag() {
        assert_eq!(SceneMode::from_immersive(true), SceneMode::Immersive);
        assert_eq!(SceneMode::from_immersive(false), SceneMode::Workstation);
    }

    // ── the gesture ─────────────────────────────────────────────────────────

    #[test]
    fn an_empty_gesture_asks_for_nothing() {
        assert!(SceneGesture::NONE.is_empty());
        assert_eq!(SceneGesture::NONE.inputs().count(), 0);
    }

    #[test]
    fn a_gesture_becomes_the_inputs_it_implies() {
        let g = SceneGesture { orbit: (4.0, -2.0), zoom: 40.0 };
        let got: Vec<_> = g.inputs().collect();
        assert_eq!(
            got,
            vec![CameraInput::Orbit { dx: 4.0, dy: -2.0 }, CameraInput::Zoom { dy: 40.0 }]
        );
    }

    /// Draining is what stops one flick of the wheel being applied on every subsequent frame.
    #[test]
    fn taking_a_gesture_leaves_nothing_behind() {
        let mut g = SceneGesture { orbit: (1.0, 2.0), zoom: 3.0 };
        assert_eq!(g.take(), SceneGesture { orbit: (1.0, 2.0), zoom: 3.0 });
        assert!(g.is_empty());
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Driving a real egui context
    //
    // egui is pure CPU — no device, no window, no display — so the whole interaction is
    // checkable **here**, in the environment that cannot run the app. These tests drive the
    // shipped `scene_viewport` inside the shape `editor_ui` actually builds (a `CentralPanel`
    // wrapping a `ScrollArea`) with a real button beside it, and assert on what reaches the
    // camera. What they cannot see is the Mac: whether the pane's rect is where the eye says it
    // is, and how the gesture feels.
    // ═════════════════════════════════════════════════════════════════════════

    const WIN: egui::Vec2 = egui::vec2(1000.0, 800.0);
    /// A control on a card — the thing a drag must never orbit through.
    const CARD_CONTROL: egui::Rect =
        egui::Rect { min: egui::Pos2 { x: 40.0, y: 40.0 }, max: egui::Pos2 { x: 240.0, y: 70.0 } };
    /// Where the workstation pane sits when nothing has been scrolled.
    const PANE: egui::Rect =
        egui::Rect { min: egui::Pos2 { x: 20.0, y: 120.0 }, max: egui::Pos2 { x: 700.0, y: 500.0 } };

    /// A headless editor: one `egui::Context`, the panel/scroll-area nesting `editor_ui` builds,
    /// and the scene region registered where the mode says it belongs.
    struct Harness {
        ctx: egui::Context,
        st: SceneInput,
        mode: SceneMode,
        pane: egui::Rect,
        /// Accumulated over every frame, because egui spreads a wheel notch across several.
        total: SceneGesture,
        /// Did the card's control fire? The half of "egui must not fight you" that is about egui
        /// keeping what is *its*.
        control_clicked: bool,
        /// What the surrounding `ScrollArea` was left to scroll with.
        scroll_left_for_the_workstation: f32,
    }

    impl Harness {
        fn new(mode: SceneMode) -> Self {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(2.0); // a Retina desk, since that is the interesting one
            Self {
                ctx,
                st: SceneInput::default(),
                mode,
                pane: PANE,
                total: SceneGesture::NONE,
                control_clicked: false,
                scroll_left_for_the_workstation: 0.0,
            }
        }

        /// Run one frame with `events` delivered to it.
        fn frame(&mut self, events: Vec<egui::Event>) {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WIN)),
                events,
                ..Default::default()
            };
            let mode = self.mode;
            let pane = self.pane;
            let st = &mut self.st;
            let mut clicked = false;
            let mut left = 0.0;
            let _ = self.ctx.clone().run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    // Immersive registers the region FIRST, under everything.
                    if mode == SceneMode::Immersive {
                        scene_viewport(ui, ui.max_rect(), mode, st);
                    }
                    // Drag-to-scroll off, mirroring `editor_ui` under a scene-behind host.
                    let mut source = egui::containers::scroll_area::ScrollSource::ALL;
                    source.drag = false;
                    egui::ScrollArea::vertical().scroll_source(source).show(ui, |ui| {
                        if ui.put(CARD_CONTROL, egui::Button::new("card control")).clicked() {
                            clicked = true;
                        }
                        // Workstation registers the region at the pane, AFTER the scroll area.
                        if mode == SceneMode::Workstation {
                            scene_viewport(ui, pane, mode, st);
                        }
                        // Whatever the scroll area would still see, read from inside its content
                        // (its own `end()` runs after this closure).
                        left = ui.ctx().input(|i| i.smooth_scroll_delta.y);
                        ui.allocate_space(egui::vec2(1.0, 2000.0)); // make the content overflow
                    });
                });
            });
            self.control_clicked |= clicked;
            // Accumulated, not overwritten: egui spreads a notch over several frames, so the
            // last frame of a settled wheel is always zero whatever happened before it.
            self.scroll_left_for_the_workstation += left;
            let g = self.st.gesture.take();
            self.total.orbit.0 += g.orbit.0;
            self.total.orbit.1 += g.orbit.1;
            self.total.zoom += g.zoom;
        }

        fn settle(&mut self) {
            // egui hit-tests against the *previous* frame's widget rects, so nothing can be
            // interacted with until the layout has been seen once.
            for _ in 0..2 {
                self.frame(vec![]);
            }
        }

        fn press(&mut self, at: egui::Pos2) {
            self.frame(vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
        }

        fn move_to(&mut self, at: egui::Pos2) {
            self.frame(vec![egui::Event::PointerMoved(at)]);
        }

        fn release(&mut self, at: egui::Pos2) {
            self.frame(vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
        }

        fn wheel(&mut self, at: egui::Pos2, lines: f32) {
            // Park the pointer first, in its own frame: egui hit-tests the current position
            // against the *previous* frame's widget rects, so a move and a wheel in one frame
            // would be answered against wherever the pointer used to be.
            self.move_to(at);
            self.frame(vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, lines),
                modifiers: Default::default(),
            }]);
            // egui smooths a notch over several frames; let it all arrive.
            for _ in 0..16 {
                self.frame(vec![]);
            }
        }

        /// A full drag: press, move in steps, release.
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.press(from);
            // Two steps, because egui only calls it a drag once the pointer has passed
            // `max_click_dist`, and because a one-frame teleport is not what a hand does.
            self.move_to(egui::pos2((from.x + to.x) * 0.5, (from.y + to.y) * 0.5));
            self.move_to(to);
            self.release(to);
        }
    }

    /// **The measurement the whole routing decision rests on.** Once `editor_ui`'s `CentralPanel`
    /// is up, egui claims the pointer *everywhere* — so `PointerRouter`, which lets egui win
    /// ties on `wants_pointer_input()`, would route every event in this host to the interface and
    /// the camera would never see one. Recorded as a test rather than a paragraph because it is
    /// the kind of claim that gets "fixed" back the other way.
    #[test]
    fn a_central_panel_makes_egui_want_every_pointer() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, WIN)),
            events: vec![egui::Event::PointerMoved(egui::pos2(900.0, 700.0))], // bare space
            ..Default::default()
        };
        // Two passes: the first lays the panel out, the second hit-tests against it.
        for _ in 0..2 {
            let _ = ctx.clone().run(raw.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("the workstation");
                });
            });
        }
        assert!(
            ctx.wants_pointer_input(),
            "if this is ever false, PointerRouter becomes usable here and this module's \
             central design note needs re-reading"
        );
    }

    /// Workstation, the ordinary gesture: a drag inside the pane orbits, in physical pixels.
    #[test]
    fn workstation_a_drag_on_the_pane_orbits() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.drag(egui::pos2(300.0, 300.0), egui::pos2(380.0, 260.0));
        // 80 points right, 40 points up, at 2× → 160 / −80 physical pixels.
        assert_eq!(h.total.orbit, (160.0, -80.0), "orbit must arrive in physical pixels");
        assert!(!h.control_clicked);
    }

    /// **The workstation trap, executed.** The pane has moved — a different rect entirely, as if
    /// the workstation had been scrolled — and the same hand movement must produce the same
    /// orbit. It does because `drag_delta` is screen-space pointer motion, which is exactly why
    /// there is no rect-and-scroll-offset mapping in this module to get wrong.
    #[test]
    fn workstation_orbit_is_unchanged_after_the_pane_has_moved() {
        let mut a = Harness::new(SceneMode::Workstation);
        a.settle();
        a.drag(egui::pos2(300.0, 300.0), egui::pos2(380.0, 260.0));

        let mut b = Harness::new(SceneMode::Workstation);
        b.pane = PANE.translate(egui::vec2(0.0, -90.0)); // scrolled up under the tab bar
        b.settle();
        b.drag(egui::pos2(300.0, 210.0), egui::pos2(380.0, 170.0)); // the same hand movement

        assert_eq!(a.total.orbit, b.total.orbit);
        assert_ne!(a.total.orbit, (0.0, 0.0));
    }

    /// A drag that starts outside the pane never reaches the camera — and the control it started
    /// on is still egui's.
    #[test]
    fn workstation_a_drag_on_a_card_control_does_not_orbit() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.drag(egui::pos2(120.0, 55.0), egui::pos2(200.0, 55.0)); // across the button
        assert_eq!(h.total.orbit, (0.0, 0.0), "a card drag must not orbit");
    }

    /// A click on a control still clicks it: the scene region senses drags only, so it is never
    /// the topmost click target.
    #[test]
    fn a_click_on_a_card_control_still_reaches_it() {
        for mode in [SceneMode::Workstation, SceneMode::Immersive] {
            let mut h = Harness::new(mode);
            h.settle();
            h.press(egui::pos2(120.0, 55.0));
            h.release(egui::pos2(120.0, 55.0));
            assert!(h.control_clicked, "{mode:?}: the interface must keep its own clicks");
            assert_eq!(h.total.orbit, (0.0, 0.0), "{mode:?}: a click is not an orbit");
        }
    }

    /// **Why the interactive filter exists**, measured rather than argued. egui registers a
    /// `WidgetRect` for **every `Ui`** — the central panel's, the scroll area's, every
    /// `ui.horizontal` — so "is any widget under the pointer" is `true` at every point in the
    /// window and an unfiltered count would freeze the immersive camera permanently. The first
    /// cut of this module used the unfiltered count and did exactly that.
    #[test]
    fn bare_scene_still_has_container_widgets_under_it() {
        let own = egui::Id::new(SCENE_VIEWPORT_ID);
        let mut h = Harness::new(SceneMode::Immersive);
        h.settle();
        h.move_to(egui::pos2(600.0, 600.0)); // bare scene: no control anywhere near
        let all = h
            .ctx
            .interaction_snapshot(|s| s.contains_pointer.iter().filter(|i| **i != own).count());
        assert!(all > 0, "if this is ever 0, the filter below can be dropped");
        assert_eq!(
            interactive_under_pointer(&h.ctx, own),
            0,
            "…but nothing there senses a gesture, which is what makes it scene"
        );
    }

    /// Immersive: the scene is the window, so a drag on bare scene orbits.
    #[test]
    fn immersive_a_drag_on_bare_scene_orbits() {
        let mut h = Harness::new(SceneMode::Immersive);
        h.settle();
        h.drag(egui::pos2(600.0, 600.0), egui::pos2(660.0, 620.0));
        assert_eq!(h.total.orbit, (120.0, 40.0));
    }

    /// Immersive: and a drag that begins on a control does not, even though the region is
    /// underneath the whole interface and egui would otherwise hand it the gesture.
    #[test]
    fn immersive_a_drag_on_a_card_control_does_not_orbit() {
        let mut h = Harness::new(SceneMode::Immersive);
        h.settle();
        h.drag(egui::pos2(120.0, 55.0), egui::pos2(220.0, 95.0));
        assert_eq!(h.total.orbit, (0.0, 0.0), "the interface's press must stay the interface's");
    }

    /// **Capture.** A drag begun on the scene keeps the camera when the pointer wanders over the
    /// interface — without it the view snaps mid-orbit, which is the whole reason
    /// `PointerRouter` was written and the property that had to survive not using it.
    #[test]
    fn a_drag_begun_on_the_scene_keeps_the_camera_over_a_control() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.press(egui::pos2(300.0, 300.0));
        h.move_to(egui::pos2(250.0, 200.0));
        assert!(h.st.is_dragging());
        h.move_to(egui::pos2(120.0, 55.0)); // over the button
        h.release(egui::pos2(120.0, 55.0));
        assert!(!h.st.is_dragging(), "the gesture must end at the release");
        // The full path, in physical pixels: (300,300) → (120,55) is (−180, −245) points.
        assert_eq!(h.total.orbit, (-360.0, -490.0), "the camera must have kept the whole gesture");
        assert!(!h.control_clicked, "and the control must not have fired under a passing drag");
    }

    /// The claim must not outlive the gesture, **including when the gesture ends nowhere near
    /// the region**. If it stuck on, the scene would own the wheel for the life of the window and
    /// the workstation would silently stop scrolling — a failure whose cause nobody would find.
    #[test]
    fn the_claim_releases_when_the_gesture_ends_off_the_region() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.press(egui::pos2(300.0, 300.0));
        h.move_to(egui::pos2(900.0, 60.0)); // dragged right off the pane
        assert!(h.st.is_dragging(), "capture must hold while the pointer is away");
        h.release(egui::pos2(900.0, 60.0)); // …and let go out there too
        assert!(!h.st.is_dragging(), "the claim must not outlive the gesture");

        // The consequence that would actually be noticed: the wheel goes back to the workstation.
        let zoom_before = h.total.zoom;
        h.wheel(egui::pos2(900.0, 60.0), 1.0);
        assert_eq!(h.total.zoom, zoom_before, "the scene must have let the wheel go");
    }

    /// The wheel over the pane zooms **and the workstation does not scroll**. Both halves are the
    /// test: leaving the delta in place is how a zoom quietly scrolls the panel it is inside.
    #[test]
    fn workstation_the_wheel_zooms_the_scene_and_not_the_panel() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.wheel(egui::pos2(300.0, 300.0), 1.0);
        assert!(h.total.zoom > 0.0, "a notch over the pane must zoom, got {}", h.total.zoom);
        assert_eq!(
            h.scroll_left_for_the_workstation, 0.0,
            "the scroll area must be left nothing to scroll with"
        );
    }

    /// And the wheel *outside* the pane still scrolls the workstation — the scene must not eat
    /// scrolling everywhere just because it is on screen.
    #[test]
    fn workstation_the_wheel_off_the_pane_still_scrolls_the_workstation() {
        let mut h = Harness::new(SceneMode::Workstation);
        h.settle();
        h.wheel(egui::pos2(850.0, 60.0), 1.0); // right of the pane, beside the card
        assert_eq!(h.total.zoom, 0.0, "the scene must not have claimed it");
        assert!(
            h.scroll_left_for_the_workstation != 0.0,
            "the scroll area must still have a delta to work with"
        );
    }
}

//! The portal — a **screen-anchored, live window into the Organon world**, floating over the
//! transcript while the transcript scrolls past underneath it.
//!
//! Everything in this module is pure: a state, an event, a rectangle derived from the pane,
//! and a pointer test. No egui state, no `World`, no `Shared`, no GPU — so the whole of the
//! portal's *decision-making* is a headless test rather than something only a machine with a
//! window server can answer. `console_main.rs` owns the texture, the render and the paint, and
//! maps the state onto them.
//!
//! # 📌 One presentation of a viewport, not the only live rectangle any more
//!
//! Tier 2b gives a [`crate::region`] a viewport of its own, and everything below is unchanged by
//! it: the verb, the state machine, the screen-anchored rect and the wheel claim are exactly what
//! they were. What changed is the *description*. A **viewport** is a producer plus a camera plus
//! a texture; the portal is one way of presenting one — floating, summoned, dismissable — and a
//! region is another — placed, persistent, arranged by hand. `SceneMode` in `scene_input` has
//! modelled that distinction since before either existed, and both of these are
//! `SceneMode::Workstation`.
//!
//! 🚨 **One mechanism, two presentations — never two implementations.** `console_main.rs` owns
//! that end: one texture, one `render_viewport`, one `paint_viewport`, one `SceneInput`
//! accumulator, one `pane_pixels_in` ratio. The two presentations differ in *which rectangle*
//! and in *who wins the frame*, and in nothing else. What that buys is stated where it matters:
//! when a second producer arrives, the portal shows it as readily as a region does, because the
//! producer seam sits below both.
//!
//! ⚠️ **The portal takes the frame from a region viewport**, and `console_main.rs`'s
//! `engine_plan` is where that is decided and argued. The loser paints a notice naming what
//! holds the frame and the command that releases it; it does not go blank and it does not show
//! a stale picture.
//!
//! # What makes it a portal rather than a patch
//!
//! Every anchor the console has today is a **scroll** anchor: [`crate::block_anchor`] pins a
//! rectangle to a run of lines, and the picture rides those lines up and off the screen. This
//! is the complement — the rectangle holds its **screen** position and the text flows under
//! it. James's words: *"the window could float in some way so that everything flows around it
//! … so when it scrolls, the window doesn't scroll away."*
//!
//! ⚠️ **The visible consequence, decided on purpose: a screen-anchored portal OCCLUDES the
//! rows behind it.** They are drawn and then covered. That is what "float" means, and it is
//! fine for something you can dismiss — which is the other half of why the CLI verb closes as
//! easily as it opens. It would not be fine as a permanent state.
//!
//! # 🚨 The portal shows the WORLD, and that is a correctness decision, not a preference
//!
//! An installed substrate rig overrides the camera **wholesale**: `world.rs`'s camera
//! finalization reads `self.substrate_rig` first and returns its whole six-tuple —
//! `(center, yaw, pitch, distance, roll, fov)` — *before* `self.yaw` / `self.pitch` /
//! `self.distance` are ever consulted. Those three are precisely what
//! `World::apply_camera_input` writes. So a portal showing the **substrate** would take a
//! drag, convert it, apply it, and draw an identical frame: no error, no log line, a green
//! build, and an investigation that starts in `scene_input.rs` — which is correct code.
//!
//! Showing the World dissolves that by construction (the World arm clears the rig), and three
//! other things fall out of the same choice for free:
//!
//! * **`organon set` / `generator` / `recipe` drive the portal with no new code at all.** That
//!   lane drains inside `World::frame_body`, which is what `render_to_texture` runs, and the
//!   console injects its own `ORGANON_IPC_NS` into every tab it spawns — so a parameter typed
//!   at a prompt *inside the console* reaches the world *inside the portal*. "Control Organon
//!   from the shell" was already built; the portal is the rectangle to see it through.
//! * **No publish-and-restore dance.** A surface has to overwrite the `Shared` snapshot with
//!   its own look and put the console's back afterwards. The portal shows the console's own
//!   snapshot, which is already published before anything renders.
//! * It is what was actually asked for — *"a little portal into a 3D world"*.
//!
//! # 🚨 The portal claims the wheel, which REVERSES a documented decision
//!
//! [`crate::block_panel::pointer_inside`] is fed `panel_placements` and not every patch, on a
//! stated rule: *"a scene patch is something to look at, so the wheel over one keeps scrolling
//! the page exactly as the wheel over a paragraph does."* The portal is the other thing. A
//! drag on it orbits and a wheel over it zooms, and neither may move the transcript
//! underneath — so it claims both.
//!
//! That is a real behavioural change to a rule that was argued, not an oversight being
//! corrected, and the argument for reversing it is one sentence: **a scene patch is a picture,
//! a portal is an instrument.** A picture that stole the wheel would be a picture that broke
//! scrolling; an instrument that did not take the wheel would be an instrument you cannot
//! reach. The patch keeps its behaviour unchanged — this claim is the portal's alone, tested
//! by [`pointer_inside`] and threaded into [`crate::term_view::draw`] beside the panel test it
//! copies.
//!
//! ⚠️ **An explicit rect test is required, and egui's layer order will not do it.**
//! `term_view` reads the wheel and every key from **raw input**, so a later-registered widget,
//! an `egui::Area` and a modal are all equally invisible to it. The precedent is
//! [`crate::block_panel::pointer_inside`], and this follows it exactly.
//!
//! # The states, and the seam
//!
//! Two states exist here: [`PortalState::Closed`] and [`PortalState::Open`]. James's design
//! has two more — an **immersive** state where the frame grows to fill the shell and the
//! overlay floats over it, and a **full screen** one beyond that, both reached by clicking and
//! both animated. Neither is built, deliberately: this tier is one visible beat.
//!
//! 📌 **The seam is [`step`] being total over `(state, event)`.** Adding immersive is adding a
//! variant and its arms, and nothing here has to be undone to do it. What must survive that
//! addition is stated where it is enforced rather than here — see `console_main.rs`'s
//! `engine_plan`: **at most one `World` render per frame, in every state.** In `Open` the
//! portal renders and the backdrop does not; in a future `Immersive` the portal *is* the
//! backdrop, so again only one renders. That invariant is free while somebody remembers why,
//! and expensive the first time it is forgotten — the two targets share `frame_index` and the
//! TAA jitter phase that rides it.

/// Where the portal is. Two states this tier; see the module docs on the seam.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PortalState {
    /// No portal. Nothing is allocated, nothing renders, and the console is exactly the
    /// terminal it was — which is the state it must start in, per James's 2026-08-11 rule
    /// that the console opens indistinguishable from an ordinary terminal.
    #[default]
    Closed,
    /// A floating, live, orbitable window onto the world.
    Open,
}

impl PortalState {
    /// Is a portal on screen? The one question `console_main.rs` asks of the state today.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

/// What moves the portal.
///
/// Only the three the CLI can send. **Deliberately no `Click` / `DoubleClick` / `Escape`**:
/// those belong to the transitions this tier does not build, and an event nothing can raise is
/// an untested arm pretending to be a design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PortalEvent {
    Open,
    Close,
    /// One verb for both directions, because it is the one a key binding or an alias wants.
    Toggle,
}

/// One event applied to the state — **pure and total**, so the whole vocabulary is a test.
///
/// `Open` on an already-open portal is deliberately the identity rather than a re-open: a
/// re-open would mean freeing and reallocating the texture (and logging that it had done so)
/// for a command that changed nothing a person could see.
pub fn step(state: PortalState, event: PortalEvent) -> PortalState {
    match event {
        PortalEvent::Open => PortalState::Open,
        PortalEvent::Close => PortalState::Closed,
        PortalEvent::Toggle => match state {
            PortalState::Closed => PortalState::Open,
            PortalState::Open => PortalState::Closed,
        },
    }
}

/// How much of the pane's width the portal spans.
///
/// Big enough that a generative world reads as a world rather than a thumbnail, small enough
/// that the transcript around it is still a transcript — the portal is a thing *in* the page,
/// and one that covered most of the page would just be the backdrop with a border on it.
pub const WIDTH_FRACTION: f32 = 0.42;

/// The portal's shape. 16:9 because it is a window onto a rendered world and that is the
/// aspect every other window onto one has.
pub const ASPECT: f32 = 16.0 / 9.0;

/// The inset from the pane's edges, in points.
pub const MARGIN: f32 = 16.0;

/// The portal's rectangle inside a pane — **derived from the pane every frame, never
/// remembered**, which is what makes it screen-anchored in the only sense that matters: it is
/// a function of where the window is now, not of anything that scrolled.
///
/// `None` for a degenerate pane. egui hands back a zero or negative rect for a frame while a
/// layout settles, and [`crate::term_view`] would then be asked whether the pointer is inside a
/// rectangle with no inside — the same defensiveness `scene_input::pane_pixels` carries, for
/// the same reason and at the same seam.
///
/// # Anchored top-right, and why that is not arbitrary
///
/// A terminal's live edge is the **bottom**: that is where new output lands and where the
/// prompt and the cursor sit. A portal anchored there would float over the one region a person
/// is actually reading and typing into. The top is where rows go to scroll away, so the portal
/// occludes the oldest visible text and never the newest. Right rather than left for the same
/// asymmetry one step smaller — output starts at the left margin.
pub fn portal_rect(pane: egui::Rect) -> Option<egui::Rect> {
    let (pw, ph) = (pane.width(), pane.height());
    if !pw.is_finite() || !ph.is_finite() || pw <= 0.0 || ph <= 0.0 {
        return None;
    }
    // The room a margin on every side leaves. A pane too small to hold a portal *and* its
    // margins gets no portal rather than a squashed one — a floating window with no gap around
    // it does not read as floating.
    let avail_w = pw - 2.0 * MARGIN;
    let avail_h = ph - 2.0 * MARGIN;
    if avail_w <= 0.0 || avail_h <= 0.0 {
        return None;
    }
    // Width first, then height from the aspect — and the height clamped back into the pane,
    // which then re-derives the width. A narrow tall pane and a wide short one both come out
    // as the same shape, which is the property that makes the aspect a *constant* rather than
    // something the layout negotiates.
    let mut w = (pw * WIDTH_FRACTION).min(avail_w);
    let mut h = w / ASPECT;
    if h > avail_h {
        h = avail_h;
        w = h * ASPECT;
    }
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let right = pane.right() - MARGIN;
    let top = pane.top() + MARGIN;
    Some(egui::Rect::from_min_size(egui::pos2(right - w, top), egui::vec2(w, h)))
}

/// Is the pointer inside the portal?
///
/// The terminal asks this before giving the wheel to the child process, exactly as it asks
/// [`crate::block_panel::pointer_inside`] about panels. `None` on either side is `false`, so a
/// console with no portal — or one nobody is pointing at — scrolls precisely as it always did.
///
/// ⚠️ **Asked of the state BEFORE the wheel is applied**, on `term_view`'s own rule: the
/// pointer is over what is on the screen right now. That is free here in a way it is not for a
/// panel, because a screen-anchored rectangle does not move when the transcript does — which
/// is worth stating rather than relying on, since it is the property that would silently stop
/// being true if the portal ever gained a scroll anchor.
pub fn pointer_inside(rect: Option<egui::Rect>, pointer: Option<egui::Pos2>) -> bool {
    match (rect, pointer) {
        (Some(r), Some(p)) => r.contains(p),
        _ => false,
    }
}

/// Is the pointer inside **any** live viewport rectangle?
///
/// 🚨 **The portal stopped being the only one, and this is the whole of what that changed on the
/// input side.** Tier 2b gives a *region* a viewport too, and §1.14 recorded the consequence in
/// advance: `term_view` reads the wheel from raw input, so the only thing that keeps a scroll out
/// of the transcript is an explicit rect test — and there is now more than one rectangle to test.
/// This is [`pointer_inside`] over a list rather than a second mechanism, for the reason §1.14
/// gives for not inventing a second gesture vocabulary: the two presentations of a viewport must
/// claim the wheel the *same* way or they will drift into claiming it differently.
///
/// An empty slice is `false`, which is the undivided, portal-less console exactly.
pub fn pointer_inside_any(rects: &[egui::Rect], pointer: Option<egui::Pos2>) -> bool {
    rects.iter().any(|r| pointer_inside(Some(*r), pointer))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane the shape the console actually runs at: the CentralPanel under a 30-point tab
    /// strip, in points.
    fn pane() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1100.0, 690.0))
    }

    /// The CLI's three verbs, over both states. `step` is total and `Open`/`Close` are
    /// **absolute** — sending the state you already want is the identity, never a churn.
    #[test]
    fn every_verb_lands_somewhere_and_open_close_are_absolute() {
        for state in [PortalState::Closed, PortalState::Open] {
            assert_eq!(step(state, PortalEvent::Open), PortalState::Open, "open from {state:?}");
            assert_eq!(step(state, PortalEvent::Close), PortalState::Closed, "close from {state:?}");
        }
        assert_eq!(
            step(PortalState::Closed, PortalEvent::Toggle),
            PortalState::Open,
            "toggle opens a closed portal"
        );
        assert_eq!(
            step(PortalState::Open, PortalEvent::Toggle),
            PortalState::Closed,
            "toggle closes an open one"
        );
    }

    /// Two toggles are the identity, which is the property that makes it a toggle rather than
    /// two verbs sharing a word.
    #[test]
    fn toggling_twice_returns_to_where_it_started() {
        for state in [PortalState::Closed, PortalState::Open] {
            let there_and_back = step(step(state, PortalEvent::Toggle), PortalEvent::Toggle);
            assert_eq!(there_and_back, state, "two toggles from {state:?}");
        }
    }

    /// The console must open as an ordinary terminal (James, 2026-08-11), so the state a
    /// `Console` is constructed with has to be `Closed` without anyone remembering to say so.
    #[test]
    fn the_default_state_is_closed() {
        assert_eq!(PortalState::default(), PortalState::Closed);
        assert!(!PortalState::default().is_open());
        assert!(PortalState::Open.is_open());
    }

    /// The rect sits inside the pane with a margin all round, at the declared fraction and
    /// aspect, anchored to the **top-right** — the corner the argument in `portal_rect`'s doc
    /// selects.
    #[test]
    fn the_rect_floats_at_the_top_right_inside_a_margin() {
        let p = pane();
        let r = portal_rect(p).expect("a 1100x690 pane holds a portal");
        assert!(p.contains_rect(r), "inside the pane: {r:?} in {p:?}");
        assert!((r.right() - (p.right() - MARGIN)).abs() < 1e-3, "margin from the right edge");
        assert!((r.top() - (p.top() + MARGIN)).abs() < 1e-3, "margin from the top edge");
        assert!((r.width() - p.width() * WIDTH_FRACTION).abs() < 1e-3, "the declared fraction");
        assert!((r.width() / r.height() - ASPECT).abs() < 1e-3, "16:9");
        assert!(r.bottom() < p.bottom(), "it floats — it does not reach the live bottom edge");
    }

    /// A pane too short for a 16:9 rect at the declared width keeps the **aspect** and gives up
    /// the width. The alternative — keeping the width and squashing — would make the aspect a
    /// thing the layout negotiates, and a world drawn at one aspect into a rect of another is
    /// the letterboxing bug this console has already avoided once.
    #[test]
    fn a_short_pane_gives_up_width_rather_than_the_aspect() {
        let p = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 200.0));
        let r = portal_rect(p).expect("a 1600x200 pane still holds one");
        assert!(p.contains_rect(r), "inside the pane");
        assert!((r.width() / r.height() - ASPECT).abs() < 1e-3, "aspect held");
        assert!(r.width() < p.width() * WIDTH_FRACTION, "width given up instead");
        assert!((r.height() - (p.height() - 2.0 * MARGIN)).abs() < 1e-3, "as tall as fits");
    }

    /// A pane egui has not laid out yet — or one mid-resize — yields no rect at all, rather
    /// than a rectangle with no inside for the pointer test to answer questions about.
    #[test]
    fn a_degenerate_pane_has_no_portal() {
        let zero = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0));
        assert_eq!(portal_rect(zero), None, "zero-sized");
        let negative = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(0.0, 0.0));
        assert_eq!(portal_rect(negative), None, "inverted");
        let sliver = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4.0, 4.0));
        assert_eq!(portal_rect(sliver), None, "smaller than its own margins");
        let nan = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(f32::NAN, 100.0));
        assert_eq!(portal_rect(nan), None, "not a number");
    }

    /// The claim itself: a point inside the portal is the portal's, a point outside it is the
    /// terminal's, and with no portal open every point is the terminal's. This is the whole of
    /// what stops a drag or a wheel over the portal from scrolling the transcript out from
    /// under it.
    #[test]
    fn the_portal_claims_points_inside_it_and_nothing_else() {
        let r = portal_rect(pane());
        assert!(pointer_inside(r, Some(r.unwrap().center())), "the middle of it");
        assert!(pointer_inside(r, Some(r.unwrap().min + egui::vec2(0.5, 0.5))), "just inside");
        assert!(!pointer_inside(r, Some(egui::pos2(10.0, 400.0))), "the transcript beside it");
        assert!(!pointer_inside(r, Some(r.unwrap().min - egui::vec2(2.0, 2.0))), "just outside");
        assert!(!pointer_inside(r, None), "the pointer has left the window");
        assert!(!pointer_inside(None, Some(egui::pos2(500.0, 100.0))), "no portal is open");
    }

    /// 🚨 **Every live viewport rectangle claims the wheel, and the empty list is the console
    /// that has none — byte for byte the behaviour before Tier 2b.**
    ///
    /// This is the pure half of §1.14's "the second consumer": `term_view` reads the wheel from
    /// raw input, so this predicate is the *only* thing standing between a scroll over a picture
    /// and the transcript underneath it. The case worth pinning hardest is the **second**
    /// rectangle — a region viewport is not the portal, does not float, and would be silently
    /// unclaimed by a test that only ever passed one rect.
    #[test]
    fn any_live_viewport_claims_the_wheel_and_an_empty_list_claims_nothing() {
        let portal = portal_rect(pane()).expect("a portal fits this pane");
        // The left half of the pane — a `3d` region beside a transcript, the shape Tier 2b runs.
        let region = egui::Rect::from_min_max(egui::pos2(0.0, 30.0), egui::pos2(550.0, 720.0));
        let both = [portal, region];

        assert!(pointer_inside_any(&both, Some(portal.center())), "the portal claims its own");
        assert!(pointer_inside_any(&both, Some(region.center())), "…and so does the region");
        // Between them and inside neither: the transcript keeps the wheel, which is the whole
        // point — claiming a rectangle must not mean claiming the window.
        assert!(
            !pointer_inside_any(&both, Some(egui::pos2(700.0, 700.0))),
            "a point in neither belongs to the transcript"
        );
        assert!(!pointer_inside_any(&both, None), "the pointer has left the window");

        // ⚠️ **The empty slice is the pre-Tier-2b console**, and it is what `console_main.rs`
        // passes when nothing is live. A predicate that answered `true` here would silently stop
        // the transcript scrolling in the default, undivided console — invariant #4's failure.
        assert!(!pointer_inside_any(&[], Some(portal.center())), "nothing is live");
        // One rectangle answers exactly as the single-rect form does, which is what makes this a
        // widening of `pointer_inside` rather than a second rule beside it.
        for p in [portal.center(), region.center(), egui::pos2(700.0, 700.0)] {
            assert_eq!(
                pointer_inside_any(&[portal], Some(p)),
                pointer_inside(Some(portal), Some(p)),
                "the list of one must agree with the single rect at {p:?}"
            );
        }
    }

    /// 📌 **egui reports `clicked()` on the FIRST click of a pair — it does not wait to see
    /// whether a second arrives.** Measured from `Response::double_clicked_by`, which is
    /// `CLICKED && button_double_clicked`, so on the second click of a pair *both* are true in
    /// one frame and there is no frame in which only the double is.
    ///
    /// The portal has no click gesture yet, and this test guards the design rather than any
    /// code here: the states James described are reached by click (portal → immersive) and by
    /// double-click (immersive → full screen), and this pins that **a single click and a
    /// double click can never mean different things in the SAME state**. His design already
    /// satisfies it — the two gestures live in different source states — and this is what
    /// catches the day someone gives a single click its own meaning in `Open`.
    #[test]
    fn a_single_click_cannot_be_made_to_wait_on_a_double() {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let at = rect.center();
        let id = egui::Id::new("portal-click-probe");

        // ⚠️ Three frames, and the first one is not padding: egui can only report an
        // interaction for a widget it already knows about, so a press delivered on the very
        // frame the widget is first registered lands on nothing at all. Frame 1 registers it
        // and parks the pointer, frame 2 presses, frame 3 releases — exactly one click, with
        // no second one anywhere in the input.
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        let frame = |events: Vec<egui::Event>| {
            let raw = egui::RawInput { events, screen_rect: Some(screen), ..Default::default() };
            let mut clicked = false;
            let mut double = false;
            ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.interact(rect, id, egui::Sense::click());
                    clicked = r.clicked();
                    double = r.double_clicked();
                });
            });
            (clicked, double)
        };
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };

        frame(vec![egui::Event::PointerMoved(at)]);
        frame(vec![press(true)]);
        let (clicked, double) = frame(vec![press(false)]);
        assert!(clicked, "the FIRST click fires immediately — nothing waits for a second");
        assert!(!double, "…and it is not reported as a double");
    }
}

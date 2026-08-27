//! **Quieting a hosted module — from the outside, because there is no way in.**
//!
//! [`organon_module::input`]'s refusal table is explicit about sound:
//!
//! > **Audio, in either direction** — §5.2. There is deliberately no path, and the design says
//! > out loud that this is *promised, not enforced* — a separate process can open WASAPI itself
//! > and Ascent already does. The grant is honoured, not prevented.
//!
//! 🚨 **So a mute verb on the protocol is exactly the wrong shape**, and this module exists to
//! avoid adding one. Every verb in that contract is a grant; a `Mute` would be the console
//! *asking* a producer to be quiet, which means a producer that ignores it is a producer the
//! console cannot silence — a control that works only while nobody minds. The console does not
//! need to ask: it **owns the child process**, so it can turn that process down in the operating
//! system's own mixer, where the answer does not depend on the module's cooperation at all.
//!
//! 📌 **The division of labour is this crate's usual one.** What lives here is the part that is
//! neither unsafe nor platform-specific: which producers are muted, where the control sits, and
//! when it is visible. The COM that reaches WASAPI lives in the root-crate binary beside the
//! other Windows calls — see `console_main.rs`'s `set_process_muted`.
//!
//! ⚠️ **Mute is the console's state, not the module's**, and it is deliberately *not* remembered
//! across launches. A muted rectangle whose muting outlived the session would be a silence
//! nobody can account for on the next run — the same argument [`crate::posture`] makes for not
//! storing a posture, and it matters more here because the symptom is *absence*.

use std::collections::BTreeSet;

/// **The control's side, in points.** Small enough to sit inside a rectangle without competing
/// with what it is showing, large enough for a pointer that is not being careful.
pub const MUTE_SIDE: f32 = 18.0;

/// How far the control is inset from the rectangle's top-right corner.
pub const MUTE_PAD: f32 = 6.0;

/// Which hosted producers the console is holding quiet.
///
/// By name, exactly as [`crate::module_input::Latch`] is, and for the same reason: a producer is
/// addressed by name everywhere else in this crate, and a second identity would be a second
/// thing to keep in step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Muted(BTreeSet<String>);

impl Muted {
    pub fn new() -> Self {
        Self::default()
    }

    /// Is this producer being held quiet?
    pub fn is(&self, producer: &str) -> bool {
        self.0.contains(producer)
    }

    /// Flip it, answering the state it is now in.
    pub fn toggle(&mut self, producer: &str) -> bool {
        if self.0.remove(producer) {
            false
        } else {
            self.0.insert(producer.to_string());
            true
        }
    }

    /// Stop tracking a producer that is no longer hosted.
    ///
    /// ⚠️ **Called when a host goes away, and it is not housekeeping.** A producer that was
    /// muted, removed, and then started again would otherwise come back silent — and nothing on
    /// screen would say why, because the control is only drawn on a rectangle that is *showing*
    /// something. A silence with no visible cause is the one failure this whole module is
    /// written against.
    pub fn forget(&mut self, producer: &str) {
        self.0.remove(producer);
    }

    /// Every producer currently held quiet, for the caller that has to apply it.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

/// **Where the control sits** — the top-right corner of the rectangle, inset.
///
/// Top-**right** because the top-left is where a producer's own chrome usually goes, and because
/// a control in the corner nearest the region divider would be the one hardest to hit in a
/// four-way split.
pub fn mute_rect(module: egui::Rect) -> egui::Rect {
    let top_right = module.right_top() + egui::vec2(-MUTE_PAD, MUTE_PAD);
    egui::Rect::from_min_max(top_right - egui::vec2(MUTE_SIDE, 0.0), top_right + egui::vec2(0.0, MUTE_SIDE))
}

/// **Should the control be drawn at all?**
///
/// 🚨 **Visible while the pointer is over the rectangle, and whenever it is muted.** Both halves
/// are the point:
///
/// * **On hover** so a playing module is an uninterrupted picture — James, 2026-08-26, on region
///   captions: a viewport is the one place that must stay clean, and a permanently-drawn button
///   is a caption with a border round it.
/// * **Always while muted** because silence is indistinguishable from a module that has nothing
///   to say. The control is the only thing on screen that can tell you the quiet was your doing,
///   and hiding it exactly when it is load-bearing would make mute a trap.
///
/// ⚠️ A rectangle too small to hold the control without covering what it is showing gets none —
/// a button occupying a quarter of a region is not a control, it is an obstruction.
pub fn show_mute(module: egui::Rect, hovered: bool, muted: bool) -> bool {
    if !(hovered || muted) {
        return false;
    }
    let need = (MUTE_SIDE + MUTE_PAD) * 3.0;
    module.width() >= need && module.height() >= need
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(w, h))
    }

    #[test]
    fn nothing_is_muted_to_begin_with() {
        let m = Muted::new();
        assert!(!m.is("ascent"));
        assert_eq!(m.iter().count(), 0);
    }

    #[test]
    fn toggling_reports_the_state_it_lands_in() {
        let mut m = Muted::new();
        assert!(m.toggle("ascent"), "the first press did not mute");
        assert!(m.is("ascent"));
        assert!(!m.toggle("ascent"), "the second press did not unmute");
        assert!(!m.is("ascent"));
    }

    /// Two producers are muted independently — one `Muted` holds a set, not a flag.
    #[test]
    fn producers_are_muted_independently() {
        let mut m = Muted::new();
        m.toggle("ascent");
        assert!(m.is("ascent") && !m.is("descent"));
        m.toggle("descent");
        assert!(m.is("ascent") && m.is("descent"));
    }

    /// 🚨 **A producer that goes away is forgotten**, or it comes back silent with nothing on
    /// screen to say why. The control is only drawn on a rectangle that is showing something, so
    /// a remembered mute on a producer that was removed and restarted is a silence with no
    /// visible cause.
    #[test]
    fn a_departed_producer_does_not_come_back_silent() {
        let mut m = Muted::new();
        m.toggle("ascent");
        m.forget("ascent");
        assert!(!m.is("ascent"), "it would restart muted");
    }

    /// 🚨 **Hidden while playing and unattended; shown whenever it is muted.** The second half is
    /// the one that matters: silence is indistinguishable from a module with nothing to say, so
    /// the control is the only thing that can attribute the quiet to a hand.
    #[test]
    fn the_control_hides_while_playing_and_stays_while_muted() {
        let r = rect(400.0, 300.0);
        assert!(!show_mute(r, false, false), "a clean playing picture grew a button");
        assert!(show_mute(r, true, false), "the control is unreachable on hover");
        assert!(show_mute(r, false, true), "a muted rectangle hid the reason it is silent");
        assert!(show_mute(r, true, true));
    }

    /// ⚠️ **A rectangle too small gets no control** — a button occupying a quarter of a region is
    /// an obstruction, not an affordance. Asserted as a *proportion* rather than against the
    /// constant, so a different size that still leaves the picture readable passes.
    #[test]
    fn a_tiny_rectangle_is_not_given_a_button() {
        let tiny = rect(40.0, 30.0);
        assert!(!show_mute(tiny, true, true), "a 40x30 region grew an 18pt button");
        // …and the control never takes more than a ninth of either side where it IS drawn.
        let ok = rect(400.0, 300.0);
        assert!(show_mute(ok, true, false));
        let c = mute_rect(ok);
        assert!(c.width() * 3.0 <= ok.width(), "the control dominates its rectangle");
    }

    /// 🚨 **The control is INSIDE the rectangle it belongs to.** A hit target that overhung the
    /// edge would take clicks meant for the region beside it — and in a four-way split that is a
    /// click landing in somebody else's viewport.
    #[test]
    fn the_control_sits_inside_its_own_rectangle() {
        for (w, h) in [(400.0, 300.0), (120.0, 120.0), (1000.0, 200.0)] {
            let r = rect(w, h);
            let c = mute_rect(r);
            assert!(r.contains_rect(c), "{w}x{h}: the control escaped its rectangle: {c:?}");
        }
    }

    /// …and in the TOP-RIGHT of it, which is the corner the geometry claims.
    #[test]
    fn the_control_is_in_the_top_right() {
        let r = rect(400.0, 300.0);
        let c = mute_rect(r);
        assert!(c.center().x > r.center().x, "not on the right: {c:?}");
        assert!(c.center().y < r.center().y, "not at the top: {c:?}");
    }
}

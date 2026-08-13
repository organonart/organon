//! Who owns the viewpoint — **the hand or the agent** — and whether anything is showing it.
//!
//! Everything here is pure: two instants, two booleans, and the two decisions they settle. No
//! egui, no `World`, no wgpu — so the policy that governs a camera an agent can move is a
//! headless test rather than something only a machine with a window server can answer.
//! `shell_main.rs` owns the clock, the `World` call and the log line, and maps these onto them.
//!
//! # 🚨 The hand always wins
//!
//! This is not a new rule and it is not this module's invention. It is the rule the lighting
//! renderer on this workstation already runs: it polls the lamp for a state it did not command,
//! and when it finds one — a hand on the app, a person reaching for the switch — it drops the
//! agent's scene and refuses new ones for a while. *A person always wins.* The reason to repeat
//! it here, in a place where it is enforced rather than remembered, is that this is the first
//! thing in the console an agent can move **while a hand is on it**: the portal's drag and wheel
//! and `organon console camera` write the same three fields, `World::apply_camera_input` cannot
//! tell them apart, and the last writer in the frame wins by accident.
//!
//! **A control that fights your hand is worse than no control.** So the arbitration is explicit,
//! it is here, and it is a test.
//!
//! # Why refuse rather than defer
//!
//! The rejected command is **dropped**, not queued. Queuing would make the camera move at a
//! moment nobody asked for it — the hold expires, and a framing chosen seconds ago arrives as a
//! jump into a shot the person has since composed themselves. That is the *same* failure the
//! hold exists to prevent, delayed. A dropped command is recoverable: an agent that meant it can
//! say it again, and the console prints why on its own stderr.
//!
//! ⚠️ **The refusal cannot travel back to the caller.** `organon console …` is fire-and-forget
//! with no return path by design (`cli::console_cmd_path`'s doc). So the agent learns nothing;
//! only a reader of the console's stderr does. That is a real gap and it is recorded in
//! `SHELL_ARCHITECTURE.md`'s honesty ledger rather than papered over here.

use std::time::{Duration, Instant};

/// How long the hand keeps the camera after its last touch.
///
/// # Why two seconds, and not the lighting renderer's thirty minutes
///
/// The lamp holds for half an hour because a hand on it expresses a **preference about the
/// room** — something that should outlive the moment it was expressed. A hand on the camera
/// expresses no such thing; it is a *motion*, and the only question is whether it is still in
/// progress. So the number has to be read as "is the hand still working", and it is bounded on
/// both sides by things that were measured rather than felt:
///
/// * **Longer than any gap _inside_ one interaction.** A drag stamps every frame (≈16 ms). A
///   wheel notch train arrives ≈100 ms apart. A hand releasing to re-grab, or letting go to look
///   before nudging again, is a few hundred milliseconds. Two seconds covers all of it, so a
///   pause in the middle of a gesture is never mistaken for the end of one.
/// * **Shorter than the time it takes to _ask_ for something.** A request reaching an agent and
///   coming back out as a command is seconds at best. So the hold has expired by the time a
///   command that was caused by the person could arrive — which is the case that must not be
///   refused, because refusing it is the feature appearing not to work.
///
/// ⚠️ **Erring long is the safer direction and it is still bounded.** A refused command costs a
/// line on stderr and a retry; a camera yanked out from under a hand costs the illusion that the
/// portal is an object you are holding. If two seconds turns out to be wrong at the machine,
/// this constant is the whole of the change.
pub const HAND_HOLD: Duration = Duration::from_secs(2);

/// What happens to an agent's framing command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Nobody's hand is on the camera — the framing is applied.
    Applied,
    /// A hand touched the camera within [`HAND_HOLD`]. The command is dropped.
    HandHolds,
}

/// Settle it. `hand_last` is when the hand last moved this camera, `None` if it never has.
///
/// 📌 **`checked_duration_since` rather than subtraction.** A stamp from the future is not
/// reachable through `Instant::now()` in one process, but the arithmetic that would panic on it
/// is one line away from a test that fabricates instants, and a panic inside a redraw is the
/// worst place in this program to discover an ordering assumption. A future stamp reads as "the
/// hand is holding", which is the safe answer.
pub fn arbitrate(hand_last: Option<Instant>, now: Instant) -> Verdict {
    match hand_last {
        None => Verdict::Applied,
        Some(t) => match now.checked_duration_since(t) {
            Some(elapsed) if elapsed >= HAND_HOLD => Verdict::Applied,
            _ => Verdict::HandHolds,
        },
    }
}

/// Is anything on screen actually showing the viewpoint a framing command moves?
///
/// # 🚨 The silent trap this exists to make audible
///
/// `World`'s camera finalization reads an installed substrate rig **first** and returns its
/// whole six-tuple before yaw, pitch and distance are consulted — and those three are exactly
/// what a framing writes. So with the backdrop on `substrate`, or with nothing showing the world
/// at all, `organon console camera --distance 40` succeeds, moves real state, and changes not one
/// pixel. That is the failure mode `portal.rs`'s module docs argue at length about, met from the
/// other side: no error, no log line, a green build, and an investigation that starts in the
/// wrong file.
///
/// The console cannot *fix* it — the camera really did move, and it will be there the moment
/// something draws the world — so it says so instead. This is the predicate that decides whether
/// to say it.
pub fn viewpoint_is_visible(portal_open: bool, backdrop_shows_world: bool) -> bool {
    portal_open || backdrop_shows_world
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera nobody has ever touched by hand is the agent's. This is the console's opening
    /// state — the portal is closed, nothing has been dragged — and it is the case that must
    /// not need a special arm to work.
    #[test]
    fn an_untouched_camera_belongs_to_whoever_asks() {
        assert_eq!(arbitrate(None, Instant::now()), Verdict::Applied);
    }

    /// The whole rule, at the three points that matter: during, at the boundary, and after.
    #[test]
    fn the_hand_holds_the_camera_for_exactly_the_hold_and_then_lets_go() {
        let now = Instant::now();
        assert_eq!(
            arbitrate(Some(now), now),
            Verdict::HandHolds,
            "the hand is on it this very frame"
        );
        assert_eq!(
            arbitrate(Some(now - HAND_HOLD / 2), now),
            Verdict::HandHolds,
            "mid-hold — a pause inside a gesture is not the end of one"
        );
        assert_eq!(
            arbitrate(Some(now - HAND_HOLD + Duration::from_millis(1)), now),
            Verdict::HandHolds,
            "one millisecond short is still the hand's"
        );
        assert_eq!(
            arbitrate(Some(now - HAND_HOLD), now),
            Verdict::Applied,
            "the boundary itself releases — the hold is a floor, not a ceiling"
        );
        assert_eq!(
            arbitrate(Some(now - Duration::from_secs(60)), now),
            Verdict::Applied,
            "a minute later the hand has plainly moved on"
        );
    }

    /// A stamp from the future must not panic inside a redraw, and the safe reading of one is
    /// "the hand is holding" — see [`arbitrate`]'s doc.
    #[test]
    fn a_stamp_from_the_future_holds_rather_than_panicking() {
        let now = Instant::now();
        assert_eq!(arbitrate(Some(now + Duration::from_secs(5)), now), Verdict::HandHolds);
    }

    /// The hold is a number chosen against measured gaps, so the properties that justify it are
    /// pinned rather than left in prose: long enough to span a pause inside a gesture, short
    /// enough that a command *caused by* the person is not refused when it arrives.
    #[test]
    fn the_hold_sits_between_a_pause_in_a_gesture_and_the_time_it_takes_to_ask() {
        assert!(
            HAND_HOLD > Duration::from_millis(500),
            "a hand releasing to re-grab must not be read as the end of the interaction"
        );
        assert!(
            HAND_HOLD <= Duration::from_secs(5),
            "a request reaching an agent and coming back is seconds; the hold must have \
             expired by then or the feature looks broken"
        );
    }

    /// The advisory's whole truth table. Either surface showing the world is enough; neither is
    /// the case worth a word on stderr.
    #[test]
    fn the_viewpoint_is_visible_through_the_portal_or_a_world_backdrop_and_nothing_else() {
        assert!(viewpoint_is_visible(true, false), "the portal alone");
        assert!(viewpoint_is_visible(false, true), "a world backdrop alone");
        assert!(viewpoint_is_visible(true, true), "both");
        assert!(
            !viewpoint_is_visible(false, false),
            "nothing is drawing the world — the framing lands somewhere invisible"
        );
    }
}

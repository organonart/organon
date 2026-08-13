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
//! ⚠️ **The refusal cannot travel back to the caller *on the CLI lane*.** `organon console …`
//! is fire-and-forget with no return path by design (`cli::console_cmd_path`'s doc). So a
//! caller on that transport learns nothing; only a reader of the console's stderr does. That
//! gap is recorded in `SHELL_ARCHITECTURE.md`'s honesty ledger rather than papered over here.
//!
//! # Reading it back — [`Viewpoint`], and why the read is MCP-only
//!
//! An agent that cannot read cannot compute a delta, which is why every framing verb is
//! absolute. Measured on 2026-08-13: asked to frame an object, an agent set a distance blind,
//! shelled out to `organon snap`, read the PNG back, judged it, and went round again — five
//! round trips and five approval prompts to compose one shot.
//!
//! The MCP server runs **in the console process** ([`crate::mcp_http`] is started from a
//! conversation tab), so that lane *can* answer. The CLI still cannot: giving `organon console`
//! a read needs a request/reply sidecar that does not exist. So the read is served as an MCP
//! capability tool and has no CLI spelling at all — deliberately, rather than as an oversight.
//!
//! 🚨 **What is published must be the live camera, never an echo of the last command.** A hand
//! on the portal outranks an agent (above), so the value an agent last *set* can already be
//! stale by the time it asks; reporting it back as current would be a confident lie. [`Viewpoint`]
//! is therefore a mirror of `World`'s own three fields, taken once per frame after **both**
//! writers have run, and [`ViewpointCell`] is the only thing the MCP thread may read.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

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

/// Who moved the viewpoint most recently.
///
/// Derived from the two stamps and nothing else, by the rule in [`last_mover`]. It is the fact
/// that makes a surprising reading intelligible: an agent that set `distance 16` and reads back
/// `520` needs to know a hand did that, or the only available conclusion is that the console is
/// broken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mover {
    /// Nobody has ever moved it — the framing is the one the window opened with.
    Nobody,
    Hand,
    Agent,
}

impl Mover {
    /// The word that goes on the wire. Spelled once, so the JSON and any future reader of it
    /// cannot come to disagree.
    pub fn as_word(self) -> &'static str {
        match self {
            Mover::Nobody => "nobody",
            Mover::Hand => "hand",
            Mover::Agent => "agent",
        }
    }
}

/// Settle who moved it last from the two stamps.
///
/// 📌 **A tie goes to the hand**, matching [`arbitrate`]: two `Instant`s equal to the nanosecond
/// is not reachable in practice, but the rule has to be *stated* rather than left to whichever
/// comparison operator was typed, and the one it should follow is already decided everywhere
/// else in this module.
pub fn last_mover(hand_last: Option<Instant>, agent_last: Option<Instant>) -> Mover {
    match (hand_last, agent_last) {
        (None, None) => Mover::Nobody,
        (Some(_), None) => Mover::Hand,
        (None, Some(_)) => Mover::Agent,
        (Some(h), Some(a)) => {
            if a > h {
                Mover::Agent
            } else {
                Mover::Hand
            }
        }
    }
}

/// One reading of where the viewer stands, as the console last measured it.
///
/// **Every field is measured** — the three axes are `World`'s own, copied; the two booleans are
/// the same two the console asks itself before it warns that a framing landed somewhere
/// invisible. Nothing here is remembered from a command: a `Viewpoint` taken after a hand has
/// dragged the portal reports where the hand put it, which is the entire point.
///
/// The two stamps are carried rather than pre-judged so that [`Self::report`] can settle
/// `hand_holds` against the clock at the moment of the *read*. Deciding it at publish time
/// would freeze a two-second hold into a snapshot that could then be minutes old.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewpoint {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    /// Measured: is the portal on screen?
    pub portal_open: bool,
    /// Measured: is the backdrop drawing the world (rather than a substrate plane, or nothing)?
    pub backdrop_shows_world: bool,
    /// When a hand last moved this camera. See [`arbitrate`].
    pub hand_last: Option<Instant>,
    /// When an *applied* agent framing last moved it. Stamped only where one is applied, so a
    /// framing the hand held off never claims to have moved anything.
    pub agent_last: Option<Instant>,
}

impl Viewpoint {
    /// The reading as JSON, for a caller that will hand it to a model.
    ///
    /// Three of the eight keys are derived, each by a rule stated where it lives:
    /// `visible` = [`viewpoint_is_visible`], `moved_by` = [`last_mover`], and `hand_holds` =
    /// [`arbitrate`] against `now`. The rest are the measured fields, copied.
    ///
    /// ⚠️ **The axes are widened exactly, never rounded.** `f64::from(0.7f32)` prints as
    /// `0.699999988079071`, which is uglier than `0.7` and is the only value that survives the
    /// round trip: a caller that reads an axis and writes it straight back must land on the
    /// same `f32`, and a tidied number would not. Rounding would also let a value sitting on a
    /// clamp boundary read as outside its own band.
    ///
    /// ⚠️ **A non-finite axis is omitted rather than serialised.** `serde_json` renders one as
    /// `null`, and a `null` where a number is expected is a value a model will try to use.
    /// `World::apply_camera_input` filters non-finite input, so this is a belt on a brace — the
    /// same one `mcp::input_schema` wears for the same reason.
    pub fn report(&self, now: Instant) -> Value {
        let mut out = Map::new();
        let mut axis = |key: &str, v: f32| {
            if let Some(n) = serde_json::Number::from_f64(f64::from(v)) {
                out.insert(key.to_string(), Value::Number(n));
            }
        };
        axis("yaw", self.yaw);
        axis("pitch", self.pitch);
        axis("distance", self.distance);
        out.insert("portal_open".into(), Value::Bool(self.portal_open));
        out.insert("backdrop_shows_world".into(), Value::Bool(self.backdrop_shows_world));
        out.insert(
            "visible".into(),
            Value::Bool(viewpoint_is_visible(self.portal_open, self.backdrop_shows_world)),
        );
        out.insert(
            "moved_by".into(),
            Value::String(last_mover(self.hand_last, self.agent_last).as_word().into()),
        );
        out.insert(
            "hand_holds".into(),
            Value::Bool(arbitrate(self.hand_last, now) == Verdict::HandHolds),
        );
        Value::Object(out)
    }
}

/// The one place a reader on another thread may learn where the camera is.
///
/// # Why a cell, and not a call into the `World`
///
/// The MCP server answers on [`crate::mcp_http`]'s serve thread; `World` lives on the UI thread
/// and is neither `Send` nor shareable. So the console *publishes* — once per frame, from the
/// frame path, after both the drained agent command and the drained hand gesture have been
/// applied. That ordering is what makes the reading the frame's own truth rather than a value
/// from halfway through it.
///
/// It is the same shape the console already uses for `Shared`: the frame path publishes, another
/// reader consumes. A snapshot cannot be stale in a way that matters here, because the camera
/// can only be moved *by a frame* — both writers run inside `redraw` — so there is no state
/// change that a published reading could be behind.
///
/// ⚠️ **One race is real and cannot be closed from this side.** An agent's *write* travels the
/// sidecar and lands on the next frame's drain, so a read issued microseconds after a write may
/// answer from the frame before it. The read is honest about what it is — the last measured
/// frame — and an agent that wants to see its own move land should read after the console has
/// drawn, not in the same breath.
#[derive(Clone, Default)]
pub struct ViewpointCell(Arc<Mutex<Option<Viewpoint>>>);

impl ViewpointCell {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called once per frame by the console. Overwrites unconditionally: there is one truth and
    /// this is it.
    pub fn publish(&self, viewpoint: Viewpoint) {
        // A poisoned lock means a thread panicked while holding it. What it guards is a `Copy`
        // struct with no invariant spanning two fields, so the value cannot be half-written and
        // the contents are as trustworthy as they ever were. Refusing to publish would turn
        // someone else's panic into a camera that silently stopped reporting.
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(viewpoint);
    }

    /// The last published reading, or `None` if the console has not drawn a frame yet.
    ///
    /// 🚨 **`None` is not a zero.** A caller must say "no reading has been taken" rather than
    /// answer with a fabricated origin — an omitted answer beats an invented one, and a framing
    /// of `(0, 0, 0)` is a viewpoint a caller would act on.
    pub fn read(&self) -> Option<Viewpoint> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
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

    // -- reading it back --------------------------------------------------

    /// The framing the window opens with, with nothing touched — `scene_input`'s defaults,
    /// spelled here rather than imported because this crate cannot see the root crate.
    fn stock() -> Viewpoint {
        Viewpoint {
            yaw: 0.7,
            pitch: 0.45,
            distance: 520.0,
            portal_open: false,
            backdrop_shows_world: false,
            hand_last: None,
            agent_last: None,
        }
    }

    /// The whole wire shape, pinned. The report is what a model reads, so its keys are a
    /// contract: a renamed one is a tool that silently stops answering the question it was
    /// called for.
    #[test]
    fn the_report_carries_the_measured_axes_and_the_three_derived_facts() {
        let now = Instant::now();
        let v = Viewpoint { portal_open: true, distance: 16.0, ..stock() };
        let report = v.report(now);

        // ⚠️ The axes are the f32s widened exactly — `0.7f32` is not `0.7`. Pinned as the
        // literal a caller would have to write back to land on the same camera.
        assert_eq!(report["yaw"], serde_json::json!(f64::from(0.7f32)));
        assert_eq!(report["pitch"], serde_json::json!(f64::from(0.45f32)));
        assert_eq!(report["distance"], serde_json::json!(16.0));
        assert_eq!(report["portal_open"], serde_json::json!(true));
        assert_eq!(report["backdrop_shows_world"], serde_json::json!(false));
        assert_eq!(report["visible"], serde_json::json!(true), "the portal alone is enough");
        assert_eq!(report["moved_by"], serde_json::json!("nobody"));
        assert_eq!(report["hand_holds"], serde_json::json!(false));
        assert_eq!(report.as_object().unwrap().len(), 8, "no key added without a test");
    }

    /// 🚨 A non-finite axis must be **absent**, never `null`. `serde_json` renders one as null,
    /// and a null where a number belongs is a value a model will try to use.
    #[test]
    fn a_non_finite_axis_is_omitted_rather_than_serialised_as_null() {
        let now = Instant::now();
        let v = Viewpoint { yaw: f32::NAN, distance: f32::INFINITY, ..stock() };
        let report = v.report(now);
        assert!(report.get("yaw").is_none(), "a NaN axis has no honest number");
        assert!(report.get("distance").is_none());
        assert_eq!(report["pitch"], serde_json::json!(f64::from(0.45f32)), "the finite one stays");
        assert!(!report.to_string().contains("null"));
    }

    /// The provenance rule, at every corner. `nobody` is the opening state and is a *different
    /// fact* from "an agent set it to the default".
    #[test]
    fn who_moved_it_last_is_settled_by_the_later_stamp_and_a_tie_goes_to_the_hand() {
        let now = Instant::now();
        let older = now - Duration::from_secs(10);
        assert_eq!(last_mover(None, None), Mover::Nobody);
        assert_eq!(last_mover(Some(now), None), Mover::Hand);
        assert_eq!(last_mover(None, Some(now)), Mover::Agent);
        assert_eq!(last_mover(Some(older), Some(now)), Mover::Agent, "the agent moved it since");
        assert_eq!(last_mover(Some(now), Some(older)), Mover::Hand, "the hand moved it since");
        assert_eq!(
            last_mover(Some(now), Some(now)),
            Mover::Hand,
            "a tie goes to the hand, as every other decision in this module does"
        );
        assert_eq!(Mover::Nobody.as_word(), "nobody");
        assert_eq!(Mover::Hand.as_word(), "hand");
        assert_eq!(Mover::Agent.as_word(), "agent");
    }

    /// 🚨 **`hand_holds` is settled against the clock at READ time, not at publish time.** The
    /// hold is two seconds and a snapshot can be older than that, so a pre-judged boolean would
    /// tell an agent a hand was on the camera long after it left.
    #[test]
    fn hand_holds_is_answered_against_the_moment_of_the_read() {
        let touched = Instant::now();
        let v = Viewpoint { hand_last: Some(touched), ..stock() };
        assert_eq!(v.report(touched)["hand_holds"], serde_json::json!(true));
        assert_eq!(
            v.report(touched + HAND_HOLD)["hand_holds"],
            serde_json::json!(false),
            "the same snapshot, read after the hold, reports the hand has let go"
        );
        // …and the provenance does NOT expire with the hold: who moved it last is a fact about
        // the past, not a claim about right now.
        assert_eq!(v.report(touched + HAND_HOLD)["moved_by"], serde_json::json!("hand"));
    }

    /// 🚨 An unpublished cell answers **nothing**, and the caller has to say so. A fabricated
    /// `(0, 0, 0)` is a viewpoint an agent would act on.
    #[test]
    fn a_cell_nobody_has_published_to_reports_no_reading_rather_than_an_origin() {
        let cell = ViewpointCell::new();
        assert_eq!(cell.read(), None);

        let now = Instant::now();
        let v = Viewpoint { distance: 16.0, ..stock() };
        cell.publish(v);
        assert_eq!(cell.read(), Some(v));

        // A second publish wins outright — there is one truth and the newest frame holds it.
        let moved = Viewpoint { distance: 40.0, hand_last: Some(now), ..stock() };
        cell.publish(moved);
        assert_eq!(cell.read(), Some(moved));
        assert_eq!(cell.read().unwrap().report(now)["moved_by"], serde_json::json!("hand"));
    }

    /// The cell is what crosses to the MCP serve thread, so it must actually cross — and a
    /// clone must see what the original published, or the console would be publishing into a
    /// copy nobody reads.
    #[test]
    fn a_clone_of_the_cell_sees_what_the_console_publishes_from_another_thread() {
        let cell = ViewpointCell::new();
        let reader = cell.clone();
        let now = Instant::now();
        let v = Viewpoint { distance: 16.0, ..stock() };
        let handle = std::thread::spawn(move || {
            cell.publish(v);
        });
        handle.join().expect("the publishing thread");
        assert_eq!(reader.read(), Some(v));
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

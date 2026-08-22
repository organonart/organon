//! **Dead, or slow? — and the rule that the answer is never "paint the last good frame".**
//!
//! §4.6 requires four distinct sentences and forbids one behaviour. This module owns the two
//! of those sentences that need something running; `organon-console`'s `module.rs` owns the
//! other two (*not approved*, *approved but not built*), which are decidable without a
//! process and are therefore not this crate's business. Between them the four rows are
//! covered exactly once each.
//!
//! # 🚨 What can and cannot be told apart from inside the protocol
//!
//! Stated plainly, because the honest version is smaller than the one people assume:
//!
//! | | The protocol can tell | How |
//! |---|---|---|
//! | quit on purpose vs. vanished | **yes** | [`crate::ProducerState::Gone`] is a farewell the producer writes |
//! | paused vs. hung | **yes** | the liveness counter is bumped once per producer *loop*, not once per *frame*, so a paused producer still moves it |
//! | starting vs. never going to start | **yes, by clock** | `Starting` is a state with a deadline, and the deadline is what turns row 3 into row 4 |
//! | **hung vs. exited without a farewell** | 🚨 **no** | both are a counter that stopped moving, and nothing inside a shared mapping distinguishes them |
//!
//! ⚠️ **The last row is the honest limit and it is not a gap this crate should close.** The
//! thing that genuinely knows a process died is the process handle, and the console holds
//! one the moment it spawns the module — that is T3b's launcher, and the answer belongs
//! there. What the heartbeat buys is that "hung" and "gone" are both distinguished from
//! *working*, within a second, without waiting on an OS notification that may never come for
//! a wedged process at all. [`Presence::Lost`] is deliberately named for what is observed —
//! the frames stopped — rather than for a cause it cannot establish.
//!
//! # The one behaviour that is forbidden, made structural
//!
//! *"Never the last good frame."* A rectangle that was rendering and now is not is exactly
//! what a broken viewport looks like, and a stale texture is the easiest wrong thing to paint
//! by accident, because the texture is still there and still valid.
//!
//! So it is not left as a discipline. [`Poll::Frame`] is **unreachable** while the producer
//! is judged `Stalled`, `Lost` or `Gone` — [`crate::ModuleChannel::poll`] does not read the
//! ring at all in those states — and [`Present`] turns the verdict into the one instruction
//! the caller's texture obeys. A call site that forgets is a call site that gets
//! [`Present::Forget`] and drops its picture anyway.

use std::time::Duration;

use crate::wire::ProducerState;

/// When a silence becomes a symptom. Every value is a **policy**, not a fact, so it is a
/// value rather than a constant — a console on a 30 Hz display and a headless test have
/// legitimately different answers, and a test that had to sleep for the real thresholds would
/// be a slow test measuring the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timings {
    /// How long [`ProducerState::Starting`] may last before it becomes [`Presence::Lost`].
    ///
    /// 🚨 §4.6: *"this one must time out into the next row rather than sitting forever."* A
    /// rectangle that says "starting…" indefinitely is the failure state that looks most like
    /// working software.
    ///
    /// Ten seconds because the thing being waited for is a process opening a mapping, not a
    /// process compiling — a module that has to be built has not reached this state at all,
    /// and `module.rs`'s *approved, not built* row covers that.
    pub start_within: Duration,
    /// How long the liveness counter may stand still before the producer is called stalled.
    pub stall_after: Duration,
    /// How long before it is called lost. Must be at least [`Timings::stall_after`].
    pub dead_after: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Timings {
            start_within: Duration::from_secs(10),
            // ~60 missed frames at 60 Hz. Long enough that a garbage-collecting or
            // shader-compiling producer is not accused, short enough that a person watching
            // a wedged viewport is told inside the time they would have started wondering.
            stall_after: Duration::from_millis(1000),
            dead_after: Duration::from_secs(5),
        }
    }
}

/// What the console believes about the producer, weighing the producer's own claim against a
/// counter and a clock. This — not [`ProducerState`] — is what a rectangle is drawn from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Launched, has not published a frame, and has not been waiting too long.
    Starting {
        /// How long it has been starting. Shown so "starting…" is visibly a process rather
        /// than a state.
        elapsed: Duration,
    },
    /// The heartbeat is moving. This covers **both** a producer publishing frames and one
    /// deliberately paused in [`crate::Lifecycle::Attached`] — from the console's point of
    /// view they are the same fact, and the difference between them is what the *frame* path
    /// says, not what the liveness path says.
    Live,
    /// The heartbeat stopped recently. Might come back.
    Stalled { silent: Duration },
    /// The heartbeat stopped long enough, or it never started in time. ⚠️ Named for the
    /// observation, not a cause — see the module docs' table.
    Lost { silent: Duration },
    /// The producer said goodbye.
    Gone,
}

impl Presence {
    /// Is the picture this producer last drew still something the console may show?
    ///
    /// 🚨 True for exactly two states, and `Stalled` is deliberately not one of them: §4.6
    /// puts *hung* in the same row as *died*, with the same instruction. The threshold is the
    /// place to argue about how patient the console is; the rule is not.
    pub fn picture_may_be_shown(&self) -> bool {
        matches!(self, Presence::Starting { .. } | Presence::Live)
    }

    /// §4.6's sentence for this state.
    ///
    /// `producer` is the name in the layout (`3d ascent`); `restart_verb` is what a person
    /// would type to try again.
    ///
    /// ⚠️ **The verb is a parameter rather than a constant here on purpose.** The console owns
    /// its command vocabulary, and a protocol crate that hardcoded `console module restart`
    /// would be a crate a *game in another repository* links in order to carry Organon's
    /// command spellings around. `module.rs` keeps `APPROVE_VERB` and `BUILD_VERB` for the
    /// same reason in the other direction: one spelling, on the side that owns it.
    pub fn sentence(&self, producer: &str, restart_verb: &str) -> String {
        match self {
            Presence::Starting { elapsed } => {
                format!("{producer} is starting — {:.0} s so far", elapsed.as_secs_f32())
            }
            Presence::Live => format!("{producer} is running"),
            Presence::Stalled { silent } => format!(
                "{producer} has stopped responding — nothing for {:.1} s. {restart_verb} to \
                 restart it",
                silent.as_secs_f32()
            ),
            Presence::Lost { silent } => format!(
                "{producer} stopped producing frames {:.0} s ago. {restart_verb} to restart it",
                silent.as_secs_f32()
            ),
            Presence::Gone => format!("{producer} has exited. {restart_verb} to start it again"),
        }
    }

    /// The verdict, from what the producer claims and what the counter and clock show.
    ///
    /// `silent` is how long the liveness counter has stood still; `since_open` is how long
    /// the console has had the channel.
    pub(crate) fn judge(
        state: ProducerState,
        published_any: bool,
        silent: Duration,
        since_open: Duration,
        t: &Timings,
    ) -> Presence {
        // A farewell outranks every clock: the producer said it was going, so no amount of
        // silence afterwards makes it a mystery.
        if state == ProducerState::Gone {
            return Presence::Gone;
        }
        if silent >= t.dead_after {
            return Presence::Lost { silent };
        }
        if !published_any && state == ProducerState::Starting {
            return if since_open >= t.start_within {
                // 🚨 Row 3 becoming row 4. The producer is still bumping its counter — it is
                // alive and simply not producing — and saying "starting" for ever would be
                // the sentence that looks most like working software.
                Presence::Lost { silent: since_open }
            } else {
                Presence::Starting { elapsed: since_open }
            };
        }
        if silent >= t.stall_after {
            return Presence::Stalled { silent };
        }
        Presence::Live
    }
}

/// The one instruction a consumer's texture obeys.
///
/// This exists so that "never the last good frame" is a *decision computed from a value*
/// rather than a rule spread across call sites — which also makes it a headless unit test on
/// a machine with no GPU, where the wgpu half cannot be exercised at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Present {
    /// A new frame is in hand; put it in the texture.
    Upload,
    /// Nothing new, and the producer is fine. Keep showing what is already there.
    Keep,
    /// 🚨 Drop the picture and draw the sentence. Never the last good frame.
    Forget,
}

/// What one poll produced. The lifetime borrows the consumer's staging buffer.
///
/// ⚠️ **There is deliberately no `Torn` variant.** A torn read means "skip this frame and keep
/// the picture", which is `Holding`'s behaviour exactly — and `region.rs`'s rule that an
/// unreachable arm is an untested branch has a dual: two arms with one behaviour is a
/// distinction the caller is asked to make and cannot act on. The event is *counted* instead,
/// on [`crate::ModuleChannel::torn_reads`], where it is diagnosable without being a decision.
#[derive(Debug)]
pub enum Poll<'a> {
    /// Launched, not yet producing. §4.6's third row.
    Starting { elapsed: Duration },
    /// A whole frame, newer than the last one this consumer took.
    Frame(crate::ring::FrameView<'a>),
    /// The producer is fine and there is nothing new — it is paused, or the console polled
    /// twice inside one of its frames.
    Holding,
    /// §4.6's fourth row, said three ways because they are three different sentences.
    Stalled { silent: Duration },
    Lost { silent: Duration },
    Gone,
}

impl Poll<'_> {
    /// What the caller's texture should do.
    pub fn present(&self) -> Present {
        match self {
            Poll::Frame(_) => Present::Upload,
            Poll::Starting { .. } | Poll::Holding => Present::Keep,
            Poll::Stalled { .. } | Poll::Lost { .. } | Poll::Gone => Present::Forget,
        }
    }

    /// The verdict this poll implies, for a rectangle that needs a sentence.
    pub fn presence(&self) -> Presence {
        match *self {
            Poll::Starting { elapsed } => Presence::Starting { elapsed },
            Poll::Frame(_) | Poll::Holding => Presence::Live,
            Poll::Stalled { silent } => Presence::Stalled { silent },
            Poll::Lost { silent } => Presence::Lost { silent },
            Poll::Gone => Presence::Gone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Timings = Timings {
        start_within: Duration::from_secs(10),
        stall_after: Duration::from_millis(1000),
        dead_after: Duration::from_secs(5),
    };

    fn s(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn a_paused_producer_is_live_because_it_still_loops() {
        // The whole reason the liveness counter is not the frame counter.
        assert_eq!(
            Presence::judge(ProducerState::Paused, true, s(0), s(60_000), &T),
            Presence::Live
        );
    }

    #[test]
    fn a_farewell_outranks_every_clock() {
        assert_eq!(
            Presence::judge(ProducerState::Gone, true, s(600_000), s(600_000), &T),
            Presence::Gone
        );
        assert_eq!(Presence::judge(ProducerState::Gone, false, s(0), s(0), &T), Presence::Gone);
    }

    #[test]
    fn starting_times_out_into_lost_rather_than_sitting_for_ever() {
        // §4.6's explicit requirement for row three.
        assert!(matches!(
            Presence::judge(ProducerState::Starting, false, s(0), s(9_999), &T),
            Presence::Starting { .. }
        ));
        assert!(matches!(
            Presence::judge(ProducerState::Starting, false, s(0), s(10_000), &T),
            Presence::Lost { .. }
        ));
    }

    #[test]
    fn a_producer_that_has_produced_is_never_called_starting_again() {
        // A live producer whose state word still says Starting — a producer that published a
        // frame before it got round to stamping Producing — must not be dragged back to row
        // three by the start deadline it has already passed.
        assert_eq!(
            Presence::judge(ProducerState::Starting, true, s(0), s(60_000), &T),
            Presence::Live
        );
    }

    #[test]
    fn silence_walks_live_to_stalled_to_lost() {
        assert_eq!(Presence::judge(ProducerState::Producing, true, s(999), s(9_999), &T), Presence::Live);
        assert!(matches!(
            Presence::judge(ProducerState::Producing, true, s(1000), s(9_999), &T),
            Presence::Stalled { .. }
        ));
        assert!(matches!(
            Presence::judge(ProducerState::Producing, true, s(5000), s(9_999), &T),
            Presence::Lost { .. }
        ));
    }

    /// 🚨 The forbidden behaviour, as a table over every state. If a state is ever added, this
    /// test is where it has to declare which side of §4.6 it falls on.
    #[test]
    fn the_last_good_frame_is_never_shown_once_the_producer_is_in_trouble() {
        assert!(Presence::Starting { elapsed: s(0) }.picture_may_be_shown());
        assert!(Presence::Live.picture_may_be_shown());
        assert!(!Presence::Stalled { silent: s(1500) }.picture_may_be_shown());
        assert!(!Presence::Lost { silent: s(9000) }.picture_may_be_shown());
        assert!(!Presence::Gone.picture_may_be_shown());
    }

    #[test]
    fn present_and_picture_may_be_shown_cannot_disagree() {
        // Two answers to one question, so they are checked against each other rather than
        // each being checked alone.
        let polls = [
            Poll::Starting { elapsed: s(1) },
            Poll::Holding,
            Poll::Stalled { silent: s(1500) },
            Poll::Lost { silent: s(9000) },
            Poll::Gone,
        ];
        for p in polls {
            let shown = p.presence().picture_may_be_shown();
            assert_eq!(
                p.present() != Present::Forget,
                shown,
                "{:?} disagrees with itself about whether the picture survives",
                p
            );
        }
    }

    #[test]
    fn every_sentence_names_the_producer_and_the_way_back() {
        for p in [
            Presence::Starting { elapsed: s(2000) },
            Presence::Stalled { silent: s(1500) },
            Presence::Lost { silent: s(9000) },
            Presence::Gone,
        ] {
            let line = p.sentence("ascent", "console module restart ascent");
            assert!(line.contains("ascent"), "{line}");
        }
        // Every state a person has to act on names the verb; the two that resolve themselves
        // do not, because a verb offered for a condition that is about to clear is noise.
        for p in [Presence::Stalled { silent: s(1500) }, Presence::Lost { silent: s(9000) }, Presence::Gone] {
            assert!(
                p.sentence("ascent", "console module restart ascent").contains("restart"),
                "{p:?} leaves the person with no way back"
            );
        }
    }
}

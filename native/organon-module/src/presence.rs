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
//! | **alive but not producing** vs. paused | **yes** | 🚨 its own section in these docs — this one nearly got away |
//! | **hung vs. exited without a farewell** | 🚨 **no** | both are a counter that stopped moving, and nothing inside a shared mapping distinguishes them |
//!
//! # 🚨 The row that nearly got away: a producer that refuses every frame
//!
//! A producer that declines to draw — because the target is a size it cannot use, because a
//! resource never loaded — is **alive and silent**. It ticks, so the liveness counter moves,
//! so every rule in that table says it is fine. And the state it most resembles is
//! [`ProducerState::Paused`], which is the **arrival state**: the least alarming conclusion
//! the console could possibly reach, about the case §4.6 most needs it to name.
//!
//! Two things close it, and it takes both:
//!
//! 1. **The producer may say so** — [`ProducerState::Refusing`] plus a [`RefusalReason`], which
//!    is what makes the sentence specific rather than merely alarmed.
//! 2. 🚨 **The console measures frame silence against the clock anyway**, separately from
//!    liveness, and only while it has asked for [`Lifecycle::Running`]. This is the
//!    load-bearing half: (1) trusts the party *least able to notice it has stopped*, and a
//!    producer wedged in its own render loop will never write the word.
//!
//! ⚠️ **The lifecycle condition is not a detail.** An `Attached` producer publishes one frame
//! and then legitimately nothing, for ever — so a frame-silence rule that ignored the
//! lifecycle would accuse every correctly-paused module within seconds of arriving.
//!
//! 📌 **A consequence worth naming, because it is the pair working as designed.** An older
//! console reading a newer producer's `Refusing` gets `None` from `ProducerState::from_wire`
//! and keeps its previous snapshot — so (1) fails silently across a version skew, which is
//! exactly the shape of failure a forward-compatible wire is *supposed* to have. (2) is
//! unaffected: the clock does not care what word the producer used, so the console still
//! reaches this verdict, just without the specific clause. A mechanism that had only (1) would
//! degrade from "stopped producing" to "healthy" at a version boundary.
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

use crate::wire::{Lifecycle, ProducerState, RefusalReason};

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
    /// How long a producer the console has asked to **run** may publish no frames before it is
    /// called [`Presence::NotProducing`].
    ///
    /// ⚠️ Measured against the **frame** counter, not the liveness counter, and applied only
    /// while the console has asked for [`Lifecycle::Running`] — see the module docs.
    ///
    /// Two seconds: long enough that a shader compile, a level load or a first-frame
    /// allocation is not accused of having stopped, short enough that a frozen viewport is
    /// named before the person looking at it starts wondering.
    pub produce_within: Duration,
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
            produce_within: Duration::from_secs(2),
        }
    }
}

/// Everything one poll observed, gathered so [`Presence::judge`] is a pure function of it.
///
/// 📌 A struct rather than seven arguments, and the reason is the tests: the interesting cases
/// here are *combinations* — refusing while stalled, running while paused, published-once then
/// silent — and a positional call of seven values is a call whose meaning nobody can check by
/// reading it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Observed {
    pub state: ProducerState,
    pub refusal: RefusalReason,
    /// Whether the producer has ever published a frame.
    pub published_any: bool,
    /// What the CONSOLE has asked for. Its own record, never read back off the wire — the
    /// question this answers is "did I ask this producer to run?", and only one party knows.
    pub asked_for: Lifecycle,
    /// How long the liveness counter has stood still.
    pub silent: Duration,
    /// How long the frame counter has stood still, restarted whenever the console asks for
    /// [`Lifecycle::Running`] so a producer is never accused of not answering a question it
    /// has not yet been asked.
    pub frames_silent: Duration,
    /// How long the console has had the channel.
    pub since_open: Duration,
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
    /// 🚨 **Alive, and no longer drawing.** §4.6's *"stopped producing"*, said as its own thing
    /// because it is the one failure whose symptom is a perfectly healthy heartbeat. `reason`
    /// is `Some` when the producer said why and `None` when the clock caught it first — and
    /// the second is not a lesser answer, it is the one that works on a producer too wedged
    /// to speak.
    NotProducing { silent: Duration, reason: Option<RefusalReason> },
    /// The heartbeat stopped long enough, or it never started in time. ⚠️ Named for the
    /// observation, not a cause — see the module docs' table.
    Lost { silent: Duration },
    /// The producer said goodbye.
    Gone,
}

impl Presence {
    /// Is the picture this producer last drew still something the console may show?
    ///
    /// 🚨 True for exactly two states, and neither `Stalled` nor `NotProducing` is one of
    /// them: §4.6 puts *hung* and *stopped producing* in the same row as *died*, with the same
    /// instruction. The thresholds are the place to argue about how patient the console is;
    /// the rule is not.
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
    ///
    /// 📌 **So `RESTART_VERB` belongs in `module.rs` beside its siblings and is *passed* from
    /// there** — decided rather than left open, because a fifth verb that existed only as an
    /// argument at one call site would sit outside the very guarantee those constants provide,
    /// which is that a sentence in a rectangle and a line a person types cannot drift apart.
    /// Both properties then hold at once: this crate spells no verb, and Organon has exactly
    /// one place where all five live. Nothing here changes when it is added.
    ///
    /// ⚠️ Whoever adds it should expect `module.rs`'s
    /// `the_verb_constants_and_the_action_words_are_one_table` to **fail**, and should want a
    /// deliberate answer rather than a mechanical fix: `restart` is a thing the console does to
    /// a producer, not a thing a person approves, so it is not one of `MODULE_ACTIONS`.
    pub fn sentence(&self, producer: &str, restart_verb: &str) -> String {
        match self {
            Presence::Starting { elapsed } => {
                format!("{producer} is starting — {:.0} s so far", elapsed.as_secs_f32())
            }
            Presence::Live => format!("{producer} is running"),
            Presence::NotProducing { silent, reason } => match reason {
                Some(r) => format!(
                    "{producer} has stopped drawing — {}. Nothing for {:.0} s. {restart_verb} to \
                     restart it",
                    r.because(),
                    silent.as_secs_f32()
                ),
                None => format!(
                    "{producer} is running but has drawn nothing for {:.0} s. {restart_verb} to \
                     restart it",
                    silent.as_secs_f32()
                ),
            },
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

    /// The verdict, from what the producer claims and what the counters and clock show.
    ///
    /// 🚨 **The order of the tests is the design**, and it is worth reading in order:
    ///
    /// 1. **A farewell outranks every clock.** The producer said it was going, so no amount of
    ///    silence afterwards makes it a mystery.
    /// 2. **Death outranks every claim.** A `Refusing` word written five seconds ago by a
    ///    process that has since stopped is a stale claim, and reporting it would name a cause
    ///    for a producer that is simply gone.
    /// 3. **Row 3's deadline**, before anything that assumes production has started.
    /// 4. **A stalled loop outranks missing frames**, because it *explains* them — reporting
    ///    "not producing" about a producer whose whole loop has stopped would name the
    ///    symptom and hide the cause. A producer that is genuinely refusing is heartbeating
    ///    fine, so it never reaches this test.
    /// 5. **Then the two halves of the refusal answer**: the producer's own word, believed
    ///    immediately; and the clock, for one that never says anything.
    pub(crate) fn judge(o: Observed, t: &Timings) -> Presence {
        if o.state == ProducerState::Gone {
            return Presence::Gone;
        }
        if o.silent >= t.dead_after {
            return Presence::Lost { silent: o.silent };
        }
        if !o.published_any && o.state == ProducerState::Starting {
            return if o.since_open >= t.start_within {
                // 🚨 Row 3 becoming row 4. The producer is still bumping its counter — it is
                // alive and simply not producing — and saying "starting" for ever would be
                // the sentence that looks most like working software.
                Presence::Lost { silent: o.since_open }
            } else {
                Presence::Starting { elapsed: o.since_open }
            };
        }
        if o.silent >= t.stall_after {
            return Presence::Stalled { silent: o.silent };
        }
        if o.state == ProducerState::Refusing {
            return Presence::NotProducing {
                silent: o.frames_silent,
                reason: Some(o.refusal),
            };
        }
        // ⚠️ Only while the console has ASKED for Running. An `Attached` producer draws one
        // frame and then legitimately nothing for ever, and that is the state every module
        // arrives in — a rule that skipped this condition would accuse each one on arrival.
        if o.asked_for == Lifecycle::Running && o.frames_silent >= t.produce_within {
            return Presence::NotProducing { silent: o.frames_silent, reason: None };
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
    /// §4.6's fourth row, said four ways because they are four different sentences.
    Stalled { silent: Duration },
    /// Alive, and no longer drawing — see [`Presence::NotProducing`], the one failure whose
    /// symptom is a perfectly healthy heartbeat.
    NotProducing { silent: Duration, reason: Option<RefusalReason> },
    Lost { silent: Duration },
    Gone,
}

impl Poll<'_> {
    /// What the caller's texture should do.
    pub fn present(&self) -> Present {
        match self {
            Poll::Frame(_) => Present::Upload,
            Poll::Starting { .. } | Poll::Holding => Present::Keep,
            Poll::Stalled { .. }
            | Poll::NotProducing { .. }
            | Poll::Lost { .. }
            | Poll::Gone => Present::Forget,
        }
    }

    /// The verdict this poll implies, for a rectangle that needs a sentence.
    pub fn presence(&self) -> Presence {
        match *self {
            Poll::Starting { elapsed } => Presence::Starting { elapsed },
            Poll::Frame(_) | Poll::Holding => Presence::Live,
            Poll::Stalled { silent } => Presence::Stalled { silent },
            Poll::NotProducing { silent, reason } => Presence::NotProducing { silent, reason },
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
        produce_within: Duration::from_secs(2),
    };

    fn s(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// A healthy producer: drawing, told to run, nothing silent. Every case in this module is it with
    /// one thing changed — which is what makes them readable, and what a positional `judge`
    /// would have cost.
    fn well() -> Observed {
        Observed {
            state: ProducerState::Producing,
            refusal: RefusalReason::Unspecified,
            published_any: true,
            asked_for: Lifecycle::Running,
            silent: s(0),
            frames_silent: s(0),
            since_open: s(60_000),
        }
    }

    #[test]
    fn a_paused_producer_is_live_because_it_still_loops() {
        // The whole reason the liveness counter is not the frame counter.
        let o = Observed {
            state: ProducerState::Paused,
            asked_for: Lifecycle::Attached,
            frames_silent: s(600_000),
            ..well()
        };
        assert_eq!(Presence::judge(o, &T), Presence::Live);
    }

    #[test]
    fn a_farewell_outranks_every_clock() {
        let o = Observed {
            state: ProducerState::Gone,
            silent: s(600_000),
            frames_silent: s(600_000),
            ..well()
        };
        assert_eq!(Presence::judge(o, &T), Presence::Gone);
        let fresh = Observed {
            state: ProducerState::Gone,
            published_any: false,
            since_open: s(0),
            ..well()
        };
        assert_eq!(Presence::judge(fresh, &T), Presence::Gone);
    }

    #[test]
    fn starting_times_out_into_lost_rather_than_sitting_for_ever() {
        // §4.6's explicit requirement for row three.
        let base = Observed {
            state: ProducerState::Starting,
            published_any: false,
            asked_for: Lifecycle::Attached,
            ..well()
        };
        assert!(matches!(
            Presence::judge(Observed { since_open: s(9_999), ..base }, &T),
            Presence::Starting { .. }
        ));
        assert!(matches!(
            Presence::judge(Observed { since_open: s(10_000), ..base }, &T),
            Presence::Lost { .. }
        ));
    }

    #[test]
    fn a_producer_that_has_produced_is_never_called_starting_again() {
        // A live producer whose state word still says Starting — one that published a frame
        // before it got round to stamping Producing — must not be dragged back to row three by
        // a deadline it has already passed.
        let o = Observed { state: ProducerState::Starting, ..well() };
        assert_eq!(Presence::judge(o, &T), Presence::Live);
    }

    #[test]
    fn silence_walks_live_to_stalled_to_lost() {
        assert_eq!(Presence::judge(Observed { silent: s(999), ..well() }, &T), Presence::Live);
        assert!(matches!(
            Presence::judge(Observed { silent: s(1000), ..well() }, &T),
            Presence::Stalled { .. }
        ));
        assert!(matches!(
            Presence::judge(Observed { silent: s(5000), ..well() }, &T),
            Presence::Lost { .. }
        ));
    }

    // -----------------------------------------------------------------------------------
    // Alive and silent — §4.6's "stopped producing"
    // -----------------------------------------------------------------------------------

    /// 🚨 The load-bearing half: the clock catches a producer that never says anything, which
    /// is the case the producer's own word cannot cover.
    #[test]
    fn a_running_producer_that_draws_nothing_is_caught_without_saying_a_word() {
        let quiet = Observed { frames_silent: s(2000), ..well() };
        assert_eq!(
            Presence::judge(quiet, &T),
            Presence::NotProducing { silent: s(2000), reason: None }
        );
        assert_eq!(
            Presence::judge(Observed { frames_silent: s(1999), ..well() }, &T),
            Presence::Live,
            "and not one moment before the deadline"
        );
    }

    /// The producer's own word is believed at once — a demonstrably live producer that says it
    /// cannot draw is not made to wait out a deadline to prove it.
    #[test]
    fn a_declared_refusal_needs_no_deadline() {
        let o = Observed {
            state: ProducerState::Refusing,
            refusal: RefusalReason::SizeUnsupported,
            frames_silent: s(0),
            ..well()
        };
        assert_eq!(
            Presence::judge(o, &T),
            Presence::NotProducing { silent: s(0), reason: Some(RefusalReason::SizeUnsupported) }
        );
    }

    /// ⚠️ The condition without which this rule would accuse **every module on arrival**:
    /// `Attached` is the state a module arrives in, and an attached producer draws once and
    /// then legitimately nothing, for ever.
    #[test]
    fn an_attached_producer_is_never_accused_of_not_drawing() {
        let o = Observed { asked_for: Lifecycle::Attached, frames_silent: s(600_000), ..well() };
        assert_eq!(Presence::judge(o, &T), Presence::Live);
    }

    /// A stalled loop **explains** missing frames, so it outranks them: naming the symptom
    /// while the cause is available would be the less useful of two true sentences.
    #[test]
    fn a_stalled_loop_outranks_missing_frames_and_a_stale_claim() {
        let o = Observed {
            state: ProducerState::Refusing,
            refusal: RefusalReason::SizeUnsupported,
            silent: s(1000),
            frames_silent: s(9000),
            ..well()
        };
        assert!(matches!(Presence::judge(o, &T), Presence::Stalled { .. }));
        // And once it is genuinely gone, the claim is not resurrected as a cause.
        assert!(matches!(
            Presence::judge(Observed { silent: s(5000), ..o }, &T),
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
        assert!(!Presence::NotProducing { silent: s(2500), reason: None }.picture_may_be_shown());
        assert!(!Presence::NotProducing {
            silent: s(0),
            reason: Some(RefusalReason::SizeUnsupported)
        }
        .picture_may_be_shown());
        assert!(!Presence::Lost { silent: s(9000) }.picture_may_be_shown());
        assert!(!Presence::Gone.picture_may_be_shown());
    }

    #[test]
    fn present_and_picture_may_be_shown_cannot_disagree() {
        // Two answers to one question, so they are checked against each other rather than each
        // being checked alone.
        let polls = [
            Poll::Starting { elapsed: s(1) },
            Poll::Holding,
            Poll::Stalled { silent: s(1500) },
            Poll::NotProducing { silent: s(2500), reason: None },
            Poll::NotProducing { silent: s(0), reason: Some(RefusalReason::SizeUnsupported) },
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
        let all = [
            Presence::Starting { elapsed: s(2000) },
            Presence::Stalled { silent: s(1500) },
            Presence::NotProducing { silent: s(2500), reason: None },
            Presence::NotProducing { silent: s(0), reason: Some(RefusalReason::SizeUnsupported) },
            Presence::Lost { silent: s(9000) },
            Presence::Gone,
        ];
        for p in all {
            let line = p.sentence("ascent", "console module restart ascent");
            assert!(line.contains("ascent"), "{line}");
        }
        // Every state a person has to act on names the verb; the one that resolves itself does
        // not, because a verb offered for a condition about to clear is noise.
        for p in all.iter().skip(1) {
            assert!(
                p.sentence("ascent", "console module restart ascent").contains("restart"),
                "{p:?} leaves the person with no way back"
            );
        }
    }

    /// A reason's words belong to the reason, so a rectangle never restates them.
    #[test]
    fn a_declared_reason_reaches_the_sentence() {
        let line =
            Presence::NotProducing { silent: s(0), reason: Some(RefusalReason::SizeUnsupported) }
                .sentence("ascent", "restart");
        assert!(line.contains(RefusalReason::SizeUnsupported.because()), "{line}");
    }
}

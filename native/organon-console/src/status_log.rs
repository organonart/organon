//! **The console's own status log — a surface of its own, and not the conversation.**
//!
//! 🚨 **The problem this exists to end.** Every line the console had to say about itself faced
//! exactly two futures: *in the conversation*, or *gone*. So chrome kept leaking back — a line
//! is judged important enough to show, and the only place to show it is James's transcript —
//! and `/trace on`, the escape hatch, made the flow **noisier** rather than opening a different
//! window. James, 2026-08-20: *"I like the idea that there is a status log somehow, but it
//! should not be present normally. … it should not feel like part of the conversational flow.
//! When everything is moving right, I generally don't care about this stuff unless there is
//! some exception or problem."*
//!
//! So the rule is a property of **where a line lives**, not of a judgement somebody has to
//! remember at the moment they write one:
//!
//! - **The log records everything, always.** [`StatusLog::push`] refuses nothing and classifies
//!   nothing. Successes included.
//! - **The conversation carries only exceptions** — [`StatusLog::exceptions`], which is
//!   [`Remark::always`] read out. A refusal, a failed send, an anomalous approvals audit, the
//!   missing-project warning.
//! - **The band carries an indicator** — [`StatusLog::slot`] — which is quiet when the log holds
//!   nothing unread and lights when it does.
//!
//! 📌 **Nothing is classified show-or-hide at the moment it is *written*, only at the moment it
//! is *drawn*.** That is the whole shape: [`Remark::always`] is one flag read by two surfaces, so
//! the conversation and the indicator cannot come to disagree about which lines are exceptional.
//!
//! ## The indicator has to be trustworthy
//!
//! 🚨 An indicator that is silent when healthy is worth having **only if it reliably lights when
//! things are not**, and this tree has repeatedly found the opposite failure — a status line that
//! cannot be wrong is not a status line. So [`StatusLog::unread`] is derived from the log's own
//! contents on every call: which lines are exceptions is a property of the lines, and the only
//! carried state is a **high-water mark** ([`StatusLog::acknowledge`]) which cannot disagree with
//! them about *which* lines exist — the sequence number of a line is its position, arithmetic
//! from the count of everything ever written and the length of what is still held.
//!
//! ⚠️ **It clears by being read, not by ageing.** Three rules were available and the middle one
//! is the only defensible one:
//!
//! - *Never clears* — a badge permanently lit is a badge nobody reads, which is the silent
//!   failure by another road.
//! - *Ages out* — the indicator would go quiet precisely because somebody stepped away, which is
//!   the case it exists for. An exception nobody has looked at is still an exception.
//! - *Clears when the log is opened* — the one event that is evidence a human looked. A new
//!   exception written afterwards lights it again, because it has not been looked at either.

use std::collections::VecDeque;

/// How many lines the log holds before the oldest falls off.
///
/// Generous on purpose: this is now the *only* home for everything that is not an exception, so
/// the cap is what a session's machinery costs rather than what a scrollback header can carry.
pub const LOG_LINES: usize = 200;

/// How many of the most recent lines the band's hover reveals.
///
/// ⚠️ **Small deliberately.** The hover is a peek, not the log — its job is to answer "is it
/// worth opening" without becoming a second, worse copy of the panel that opens.
pub const HOVER_LINES: usize = 3;

/// One line of the console's own log, and whether a person sees it **without opening the log**.
///
/// 🚨 **`always` is the quiet/loud rule made per-line rather than per-caller**, and the default is
/// deliberately the loud one: [`crate::conversation_view::ConversationPane::note`] keeps its
/// signature and its meaning, so a line written by somebody who did not think about this is
/// **seen**. A surface whose default is silence is a surface that eventually swallows the one
/// message that mattered. [`crate::conversation_view::ConversationPane::trace`] is the opt-in for
/// the other half.
///
/// ⚠️ **The word still means what it says, and it now means one thing more.** `always` = drawn in
/// the conversation whatever the mode — *and* the thing that puts the band's indicator into its
/// attention state. One field, two surfaces, by construction rather than by two lists that
/// happen to agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remark {
    pub text: String,
    /// True = an exception: drawn in the conversation, and it lights the band's indicator.
    /// False = machinery: the log holds it and nothing else shows it.
    pub always: bool,
}

/// Everything the band's indicator needs, decided before anything is laid out.
///
/// Absent — `Option::None` at the call site — means the log is **empty**, and an empty log has
/// no indicator at all. That is not the same as a quiet one: there is nothing to open, so
/// offering a control that opens nothing would be the band asserting a surface that does not
/// exist yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogSlot {
    /// How many lines the log holds. Never zero — see the type's own note.
    pub lines: usize,
    /// Whether the log holds an exception written since it was last opened.
    pub attention: bool,
    /// How many such exceptions. Zero exactly when [`Self::attention`] is false.
    pub unread: usize,
    /// The most recent lines, **oldest first**, for the hover. At most [`HOVER_LINES`].
    pub latest: Vec<String>,
}

/// The indicator's quiet face: present, hoverable, clickable, saying nothing.
pub const LOG_QUIET: &str = "log";

/// The indicator's attention face.
///
/// ⚠️ **`●` (U+25CF) is a deliberate reuse, not a fresh pick.** egui's PROPORTIONAL face carries
/// neither it nor the band's `◈`, so both are drawn `.monospace()` — the same tofu fix the
/// reading and the approval card already carry. A prettier glyph chosen here would come back as
/// a box on the one face this band is drawn in.
pub const LOG_ATTENTION: &str = "● log";

/// What the indicator reads, given the slot. Pure, so the two faces can be pinned by value.
pub fn log_label(slot: &LogSlot) -> &'static str {
    if slot.attention {
        LOG_ATTENTION
    } else {
        LOG_QUIET
    }
}

/// The console's own remarks about one session, and how much of them has been read.
///
/// ⚠️ **A type rather than a `VecDeque` on the pane**, because the interesting part is the
/// unread arithmetic and testing it through a real pane would mean spawning an agent process to
/// find out whether a dot is lit.
#[derive(Clone, Debug, Default)]
pub struct StatusLog {
    lines: VecDeque<Remark>,
    /// Everything ever pushed, including what has since fallen off the front. Monotonic, and it
    /// is what gives a line a stable identity without a field on [`Remark`] that every literal
    /// in this tree would have to name.
    written: u64,
    /// [`Self::written`] as it stood the last time a human opened the log.
    read_through: u64,
}

impl StatusLog {
    /// Record a line. Nothing is refused and nothing is classified here — see the module doc.
    pub fn push(&mut self, remark: Remark) {
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(remark);
        self.written += 1;
    }

    /// Every line, oldest first — **including** the ones no other surface is drawing.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Remark> {
        self.lines.iter()
    }

    /// The lines the **conversation** draws: the exceptions, in order.
    pub fn exceptions(&self) -> impl Iterator<Item = &Remark> {
        self.lines.iter().filter(|remark| remark.always)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The most recent line, whatever it is. Tests and diagnostics; no surface reads it.
    pub fn last(&self) -> Option<&Remark> {
        self.lines.back()
    }

    /// The sequence number of the oldest line still held.
    ///
    /// Arithmetic rather than a stored field, which is the point: it cannot disagree with the
    /// deque about which lines exist, because it is computed from the deque.
    fn first_seq(&self) -> u64 {
        self.written - self.lines.len() as u64
    }

    /// **How many exceptions have arrived since the log was last opened.**
    ///
    /// Derived on every call from the lines themselves. The only carried numbers are two
    /// counters, and neither of them decides *which* lines are exceptional.
    pub fn unread(&self) -> usize {
        let first = self.first_seq();
        self.lines
            .iter()
            .enumerate()
            .filter(|(i, remark)| remark.always && first + *i as u64 >= self.read_through)
            .count()
    }

    /// Whether the band's indicator is in its attention state.
    pub fn attention(&self) -> bool {
        self.unread() > 0
    }

    /// Mark everything written so far as read. Called when the log is **opened**, and nowhere
    /// else — see the module doc on why not on a timer.
    pub fn acknowledge(&mut self) {
        self.read_through = self.written;
    }

    /// What the band draws, or `None` when there is nothing to draw it about.
    pub fn slot(&self) -> Option<LogSlot> {
        if self.lines.is_empty() {
            return None;
        }
        let unread = self.unread();
        let latest = self
            .lines
            .iter()
            .skip(self.lines.len().saturating_sub(HOVER_LINES))
            .map(|remark| remark.text.clone())
            .collect();
        Some(LogSlot { lines: self.lines.len(), attention: unread > 0, unread, latest })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> Remark {
        Remark { text: text.to_string(), always: true }
    }

    fn machinery(text: &str) -> Remark {
        Remark { text: text.to_string(), always: false }
    }

    /// The premise of the whole surface: **nothing is refused at write time.**
    #[test]
    fn the_log_records_the_machinery_and_the_exception_alike() {
        let mut log = StatusLog::default();
        log.push(machinery("ok /viewport center agent"));
        log.push(note("could not send: broken pipe"));
        assert_eq!(log.len(), 2, "the log dropped a line somebody may need");
        assert_eq!(
            log.exceptions().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["could not send: broken pipe"],
            "the conversation is drawing machinery, which is the leak this change closes",
        );
    }

    /// 🚨 **The indicator's honest direction — it must LIGHT.** An indicator that is silent when
    /// healthy is only worth having if it reliably says so when things are not.
    #[test]
    fn an_exception_lights_the_indicator() {
        let mut log = StatusLog::default();
        log.push(machinery("stderr: Warning: no stdin data received"));
        assert!(!log.attention(), "machinery lit the indicator — it will stop being read");
        log.push(note("`/organon motion` was refused: no region holds a stack"));
        assert!(log.attention(), "an exception left the indicator dark — the badge is a lie");
        assert_eq!(log.unread(), 1);
    }

    /// The other direction, and the reason it is a high-water mark rather than a flag: opening
    /// the log clears it, and the **next** exception lights it again.
    #[test]
    fn opening_the_log_clears_it_and_the_next_exception_lights_it_again() {
        let mut log = StatusLog::default();
        log.push(note("could not send: broken pipe"));
        assert!(log.attention());
        log.acknowledge();
        assert!(!log.attention(), "the badge survived being read — a badge that never clears is noise");
        log.push(machinery("ok /theme organon"));
        assert!(!log.attention(), "machinery lit an acknowledged log");
        log.push(note("the agent process ended"));
        assert!(log.attention(), "an exception after the log was read did not light it");
        assert_eq!(log.unread(), 1, "the acknowledged one came back");
    }

    /// ⚠️ **The arithmetic under the cap, which is where a stored index would have gone wrong.**
    /// A line's identity is its sequence number, so lines falling off the front must not shift
    /// what is unread.
    #[test]
    fn the_unread_count_survives_the_cap() {
        let mut log = StatusLog::default();
        for i in 0..LOG_LINES {
            log.push(machinery(&format!("line {i}")));
        }
        log.acknowledge();
        log.push(note("an exception"));
        // Every push from here drops one line off the front.
        for i in 0..LOG_LINES {
            log.push(machinery(&format!("after {i}")));
            assert!(
                log.len() <= LOG_LINES,
                "the cap stopped holding at {i}",
            );
        }
        assert_eq!(log.len(), LOG_LINES);
        assert!(
            !log.attention(),
            "the exception has fallen off the front, so there is nothing left to point at",
        );
        log.push(note("a fresh one"));
        assert!(log.attention(), "a fresh exception did not light a full log");
        assert_eq!(log.unread(), 1);
    }

    /// An empty log has no indicator: there is nothing to open.
    #[test]
    fn an_empty_log_offers_no_indicator() {
        let log = StatusLog::default();
        assert!(log.slot().is_none(), "the band offered a control that opens nothing");
    }

    /// The hover is a peek, oldest-first, and bounded.
    #[test]
    fn the_hover_shows_the_last_few_lines_in_reading_order() {
        let mut log = StatusLog::default();
        for i in 0..10 {
            log.push(machinery(&format!("line {i}")));
        }
        let slot = log.slot().expect("ten lines is not none");
        assert_eq!(slot.lines, 10);
        assert_eq!(slot.latest, ["line 7", "line 8", "line 9"], "the peek is not in reading order");
        assert_eq!(slot.latest.len(), HOVER_LINES);
        assert_eq!(log_label(&slot), LOG_QUIET, "a quiet log drew the attention face");
    }

    /// The two faces, pinned by value — the band's own test measures one of them for width.
    #[test]
    fn the_indicator_has_exactly_two_faces() {
        let mut log = StatusLog::default();
        log.push(machinery("quiet"));
        assert_eq!(log_label(&log.slot().expect("one line")), "log");
        log.push(note("loud"));
        assert_eq!(log_label(&log.slot().expect("two lines")), "● log");
    }
}

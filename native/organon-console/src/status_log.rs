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
//! - **One permanent line at the top of the pane carries the summary** —
//!   [`StatusLog::summary`] — and clicking it drops the whole log down over the page.
//!
//! 📌 **Nothing is classified show-or-hide at the moment it is *written*, only at the moment it
//! is *drawn*.** That is the whole shape: [`Remark::always`] is one flag read by two surfaces, so
//! the conversation and the summary cannot come to disagree about which lines are exceptional.
//!
//! ## The summary has to be trustworthy
//!
//! 🚨 A status line that is quiet when healthy is worth having **only if it reliably says so when
//! things are not**, and this tree has repeatedly found the opposite failure — a status line that
//! cannot be wrong is not a status line. So [`StatusLog::summary`] is derived from the log's own
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
//! - *Ages out* — the summary would go quiet precisely because somebody stepped away, which is
//!   the case it exists for. An exception nobody has looked at is still an exception.
//! - *Clears when the log is opened* — the one event that is evidence a human looked. A new
//!   exception written afterwards lights it again, because it has not been looked at either.
//!
//! ⚠️ **[`Health::Warning`] is what the middle rule costs, and it is why there are three states
//! rather than two.** Once the reader has looked, "you have unread exceptions" is false — but
//! "nothing has gone wrong this session" is *also* false, and collapsing back to
//! [`Health::Ok`] would be the log telling a comfortable lie about a session that broke twenty
//! minutes ago. So an acknowledged exception leaves the line amber rather than green.
//!
//! ## Every line carries the time it was written
//!
//! ⚠️ **Local wall clock, `HH:MM:SS`, and the choice is against elapsed-since-start.** What a
//! reader does with a status line is correlate it against something outside the console — a
//! terminal they were watching, a file's mtime, their own memory of "it was about ten past" —
//! and every one of those is wall clock. Elapsed-since-start reads well *within* a session and
//! is useless the moment the question leaves it, because it requires knowing when the session
//! began, which is the one fact nobody remembers.
//!
//! ⚠️ **The date is on the panel's header, not on every row**, and that is what a session
//! spanning midnight costs. `00:07:03` sitting under `23:58:11` is unreadable if the surface has
//! never said what day it is; so [`StatusLog::date_span`] names the day, or both days, and the
//! header carries it. The rows stay eight characters wide either way, which is what keeps them
//! a column rather than a sentence.

use std::collections::VecDeque;

/// How many lines the log holds before the oldest falls off.
///
/// Generous on purpose: this is now the *only* home for everything that is not an exception, so
/// the cap is what a session's machinery costs rather than what a scrollback header can carry.
pub const LOG_LINES: usize = 200;

/// A **local civil time**, resolved once at the moment a line is written.
///
/// ⚠️ **Plain integers rather than a `DateTime`**, and that is the testability decision: a
/// literal of this struct is a timestamp a test can pin, so every formatting rule below —
/// including the midnight case, which nobody can wait for — is exercised with values rather
/// than with whatever the clock happened to say. [`LogTime::now`] is the only place in this
/// crate that reads a clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LogTime {
    /// The local time now.
    ///
    /// ⚠️ **`Local`, not `Utc`.** A console line timestamped in UTC is a line whose reader has to
    /// do arithmetic before it can be compared with anything else on their screen, and a reader
    /// doing arithmetic on a status line is a reader who stops reading it.
    pub fn now() -> Self {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        Self {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            second: now.second(),
        }
    }

    /// `HH:MM:SS` — the log's timestamp column. Fixed width by construction, which is what
    /// makes the marks and the text beyond it line up into a column at all.
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// `YYYY-MM-DD` — the panel header's answer to "which day is this?".
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Whether two stamps fall on the same local day. The whole of the midnight rule.
    pub fn same_day(&self, other: &Self) -> bool {
        (self.year, self.month, self.day) == (other.year, other.month, other.day)
    }
}

/// One line of the console's own log: what it said, whether a person sees it **without opening
/// the log**, and when it was written.
///
/// 🚨 **`always` is the quiet/loud rule made per-line rather than per-caller**, and the default is
/// deliberately the loud one: [`crate::conversation_view::ConversationPane::note`] keeps its
/// signature and its meaning, so a line written by somebody who did not think about this is
/// **seen**. A surface whose default is silence is a surface that eventually swallows the one
/// message that mattered. [`crate::conversation_view::ConversationPane::trace`] is the opt-in for
/// the other half.
///
/// ⚠️ **The word still means what it says, and it now means one thing more.** `always` = drawn in
/// the conversation whatever the mode — *and* the thing that puts the pane's status line into its
/// attention state. One field, two surfaces, by construction rather than by two lists that
/// happen to agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remark {
    pub text: String,
    /// True = an exception: drawn in the conversation, and it lights the pane's status line.
    /// False = machinery: the log holds it and nothing else shows it.
    pub always: bool,
    /// When it was written, in local civil time. See the module doc for why wall clock.
    ///
    /// ⚠️ **Stamped at construction, never at draw time.** A timestamp taken when a line is
    /// *rendered* is a timestamp that changes every frame, which is the one thing a trace log
    /// may not do.
    pub at: LogTime,
}

impl Remark {
    /// An exception, stamped now — the conversation sees it too.
    pub fn note(text: impl Into<String>) -> Self {
        Self { text: text.into(), always: true, at: LogTime::now() }
    }

    /// Machinery, stamped now — the log holds it and nothing else shows it.
    pub fn machinery(text: impl Into<String>) -> Self {
        Self { text: text.into(), always: false, at: LogTime::now() }
    }
}

/// **How the console is doing**, in the three states a colour can carry.
///
/// 🚨 Three, not two, and the middle one is the whole reason: see the module doc on what
/// acknowledging an exception must *not* do. Each maps onto a field the [`crate::theme::Theme`]
/// already owns — no palette is invented for this surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// Nothing has gone wrong. `Theme::ok`.
    Ok,
    /// Something went wrong and has been read. `Theme::asking`.
    Warning,
    /// Something has gone wrong that nobody has looked at. `Theme::bad`.
    Attention,
}

/// Everything the pane's permanent status line needs, derived from the log on every call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogSummary {
    pub health: Health,
    /// The one line the status line draws. Never empty — a status line with nothing in it is
    /// indistinguishable from a console that has stopped drawing.
    pub text: String,
    /// How many lines the log holds. May be zero.
    pub lines: usize,
    /// How many of them are exceptions.
    pub exceptions: usize,
    /// How many exceptions have arrived since the log was last opened.
    pub unread: usize,
}

/// The mark on the status line when the log is **closed**, and when it is **open**.
///
/// ⚠️ **ASCII, deliberately** — the same pair [`crate::card_density::MORE`] and `LESS` settled on,
/// and for the identical reason: every disclosure triangle anyone reaches for (`▸`, `▾`, `⌄`) is
/// in none of the four fonts egui bundles, so it draws as a box. The glyph guard in
/// `conversation_view` walks these strings.
pub const DROP_CLOSED: &str = "+";
/// See [`DROP_CLOSED`].
pub const DROP_OPEN: &str = "-";

/// Which mark the status line draws. Pure, so the two faces can be pinned by value.
pub fn drop_mark(open: bool) -> &'static str {
    if open {
        DROP_OPEN
    } else {
        DROP_CLOSED
    }
}

/// The mark on an exception's row, and the one on everything else.
///
/// ⚠️ Both are drawn `.monospace()` for the band's tofu reason, and they are the same **width**
/// on that face, which is what keeps the text of every row on one left edge. A ragged gutter in a
/// log is what makes it unreadable at a glance, which is the only way anybody reads a log.
pub const LOG_MARK_EXCEPTION: &str = "●";
/// See [`LOG_MARK_EXCEPTION`].
pub const LOG_MARK_QUIET: &str = "·";

/// The console's own remarks about one session, and how much of them has been read.
///
/// ⚠️ **A type rather than a `VecDeque` on the pane**, because the interesting part is the
/// unread arithmetic and testing it through a real pane would mean spawning an agent process to
/// find out whether a line is amber.
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
    pub fn exceptions(&self) -> impl DoubleEndedIterator<Item = &Remark> {
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

    /// Whether the pane's status line is in its attention state.
    pub fn attention(&self) -> bool {
        self.unread() > 0
    }

    /// Mark everything written so far as read. Called when the log is **opened**, and nowhere
    /// else — see the module doc on why not on a timer.
    pub fn acknowledge(&mut self) {
        self.read_through = self.written;
    }

    /// The newest exception the reader has not seen, if there is one.
    fn newest_unread_exception(&self) -> Option<&Remark> {
        let first = self.first_seq();
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find(|(i, remark)| remark.always && first + *i as u64 >= self.read_through)
            .map(|(_, remark)| remark)
    }

    /// **What the permanent status line says**, derived from the lines on every call.
    ///
    /// 🚨 The health is a function of the log's contents and the high-water mark and of nothing
    /// else. There is no flag anybody sets, which is what stops it from being a status line that
    /// cannot be wrong.
    pub fn summary(&self) -> LogSummary {
        let unread = self.unread();
        let exceptions = self.exceptions().count();
        let health = if unread > 0 {
            Health::Attention
        } else if exceptions > 0 {
            Health::Warning
        } else {
            Health::Ok
        };
        let text = match health {
            // The newest thing wrong, in its own words. A count on its own would be a status
            // line that makes you open a panel to learn what it is about.
            Health::Attention => {
                let newest = self.newest_unread_exception().map(|r| r.text.as_str()).unwrap_or("");
                if unread == 1 {
                    newest.to_string()
                } else {
                    format!("{unread} new · {newest}")
                }
            }
            Health::Warning => {
                let plural = if exceptions == 1 { "exception" } else { "exceptions" };
                format!("{exceptions} {plural} this session · all read")
            }
            // ⚠️ Two different true sentences, because "0 lines · all clear" reads as a console
            // that is not reporting rather than one with nothing to report.
            Health::Ok if self.lines.is_empty() => "nothing to report".to_string(),
            Health::Ok => {
                let plural = if self.lines.len() == 1 { "line" } else { "lines" };
                format!("all clear · {} {plural}", self.lines.len())
            }
        };
        LogSummary { health, text, lines: self.lines.len(), exceptions, unread }
    }

    /// **Which local day, or days, the log covers** — the panel header's line, and the whole of
    /// what a session spanning midnight is given.
    ///
    /// `None` for an empty log: a header cannot name the day of nothing.
    pub fn date_span(&self) -> Option<String> {
        let first = self.lines.front()?.at;
        let last = self.lines.back()?.at;
        if first.same_day(&last) {
            Some(first.date())
        } else {
            // ⚠️ `→` U+2192 is on the glyph allowlist and is drawn `.monospace()`, which is the
            // face Hack carries it in.
            Some(format!("{} → {}", first.date(), last.date()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed stamp, so nothing here depends on the clock.
    fn at(hour: u32, minute: u32, second: u32) -> LogTime {
        LogTime { year: 2026, month: 8, day: 21, hour, minute, second }
    }

    fn note(text: &str) -> Remark {
        Remark { text: text.to_string(), always: true, at: at(12, 0, 0) }
    }

    fn machinery(text: &str) -> Remark {
        Remark { text: text.to_string(), always: false, at: at(12, 0, 0) }
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

    /// 🚨 **The summary's honest direction — it must LIGHT.** A status line that is quiet when
    /// healthy is only worth having if it reliably says so when things are not.
    ///
    /// ⚠️ **Mutation-checked, both ways.** Make [`StatusLog::unread`]'s filter ignore `always`
    /// and the first assertion fails with *"machinery lit the status line — it will stop being
    /// read"*; make [`StatusLog::summary`]'s health always [`Health::Ok`] and the second fails
    /// with *"an exception left the status line green — the summary is a lie"*.
    #[test]
    fn an_exception_lights_the_status_line() {
        let mut log = StatusLog::default();
        log.push(machinery("stderr: Warning: no stdin data received"));
        assert_eq!(
            log.summary().health,
            Health::Ok,
            "machinery lit the status line — it will stop being read",
        );
        log.push(note("`/organon motion` was refused: no region holds a stack"));
        assert_eq!(
            log.summary().health,
            Health::Attention,
            "an exception left the status line green — the summary is a lie",
        );
        assert_eq!(log.summary().unread, 1);
        assert_eq!(
            log.summary().text,
            "`/organon motion` was refused: no region holds a stack",
            "one unread exception says what it is rather than counting itself",
        );
    }

    /// 🚨 **The middle state, and the reason there are three.** Opening the log clears the
    /// *attention*; it does not make the session's exception un-happen.
    ///
    /// ⚠️ **Mutation-checked**: collapse [`Health::Warning`] into [`Health::Ok`] — i.e. drop the
    /// `exceptions > 0` arm — and this fails with *"a session that broke is not 'all clear'"*.
    #[test]
    fn an_acknowledged_exception_leaves_the_line_amber_and_never_green() {
        let mut log = StatusLog::default();
        log.push(note("could not send: broken pipe"));
        assert_eq!(log.summary().health, Health::Attention);
        log.acknowledge();
        let read = log.summary();
        assert_eq!(read.unread, 0, "the badge survived being read — a badge that never clears is noise");
        assert_eq!(
            read.health,
            Health::Warning,
            "a session that broke is not 'all clear' — {}",
            read.text,
        );
        assert_eq!(read.text, "1 exception this session · all read");
        log.push(machinery("ok /theme organon"));
        assert_eq!(log.summary().health, Health::Warning, "machinery lit an acknowledged log");
        log.push(note("the agent process ended"));
        let fresh = log.summary();
        assert_eq!(fresh.health, Health::Attention, "an exception after the log was read did not light it");
        assert_eq!(fresh.unread, 1, "the acknowledged one came back");
        assert_eq!(fresh.exceptions, 2, "…and the read one is still counted as having happened");
    }

    /// Several unread exceptions count themselves and still name the newest.
    #[test]
    fn several_unread_exceptions_count_themselves_and_name_the_newest() {
        let mut log = StatusLog::default();
        log.push(note("could not send: broken pipe"));
        log.push(machinery("ok /theme organon"));
        log.push(note("the agent process ended"));
        let summary = log.summary();
        assert_eq!(summary.unread, 2);
        assert_eq!(summary.text, "2 new · the agent process ended");
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
        // …and with it gone, so is the session's memory of it: the summary reports the lines it
        // HOLDS, which is the only thing it can honestly report.
        assert_eq!(log.summary().health, Health::Ok);
        log.push(note("a fresh one"));
        assert!(log.attention(), "a fresh exception did not light a full log");
        assert_eq!(log.unread(), 1);
    }

    /// An empty log is green and says so in words.
    #[test]
    fn an_empty_log_reads_as_nothing_to_report() {
        let log = StatusLog::default();
        let summary = log.summary();
        assert_eq!(summary.health, Health::Ok);
        assert_eq!(summary.text, "nothing to report");
        assert_eq!(summary.lines, 0);
        assert!(!summary.text.is_empty(), "a status line is never blank");
        assert_eq!(log.date_span(), None, "a header cannot name the day of nothing");
    }

    /// A quiet log with lines in it says how many.
    #[test]
    fn a_quiet_log_counts_its_lines() {
        let mut log = StatusLog::default();
        log.push(machinery("one"));
        assert_eq!(log.summary().text, "all clear · 1 line");
        log.push(machinery("two"));
        assert_eq!(log.summary().text, "all clear · 2 lines");
    }

    /// 🚨 **The timestamp column is fixed width**, which is the whole of what makes the rows a
    /// column rather than a paragraph.
    #[test]
    fn the_clock_is_eight_characters_at_every_hour() {
        for (h, m, s) in [(0, 0, 0), (9, 5, 3), (23, 59, 59), (12, 0, 0)] {
            let stamp = at(h, m, s);
            assert_eq!(stamp.clock().len(), 8, "{stamp:?} is not eight characters");
        }
        assert_eq!(at(0, 0, 0).clock(), "00:00:00");
        assert_eq!(at(9, 5, 3).clock(), "09:05:03");
        assert_eq!(at(23, 59, 59).clock(), "23:59:59");
    }

    /// 🚨 **What a session spanning midnight costs, and what is done about it.**
    ///
    /// The rows only ever say `HH:MM:SS`, so `00:07:03` under `23:58:11` would be unreadable if
    /// nothing named the day. The header names it — one date for an ordinary session, both for
    /// one that crossed.
    #[test]
    fn a_session_that_crosses_midnight_is_named_on_the_header() {
        let mut log = StatusLog::default();
        log.push(Remark {
            text: "late".into(),
            always: false,
            at: LogTime { year: 2026, month: 8, day: 21, hour: 23, minute: 58, second: 11 },
        });
        assert_eq!(log.date_span().as_deref(), Some("2026-08-21"), "one day, one date");
        log.push(Remark {
            text: "later".into(),
            always: false,
            at: LogTime { year: 2026, month: 8, day: 22, hour: 0, minute: 7, second: 3 },
        });
        assert_eq!(
            log.date_span().as_deref(),
            Some("2026-08-21 → 2026-08-22"),
            "a log whose rows go backwards must say why",
        );
        assert!(!at(23, 58, 11).same_day(&LogTime {
            year: 2026,
            month: 8,
            day: 22,
            hour: 0,
            minute: 7,
            second: 3
        }));
    }

    /// The two disclosure faces, pinned by value.
    #[test]
    fn the_status_line_has_exactly_two_disclosure_faces() {
        assert_eq!(drop_mark(false), "+");
        assert_eq!(drop_mark(true), "-");
    }

    /// 📌 The clock is real and is local. Nothing is asserted about *what* it says — only that
    /// it produces a well-formed stamp, which is the part a test can know.
    #[test]
    fn now_produces_a_well_formed_local_stamp() {
        let now = LogTime::now();
        assert!(now.year >= 2020, "{now:?}");
        assert!((1..=12).contains(&now.month), "{now:?}");
        assert!((1..=31).contains(&now.day), "{now:?}");
        assert!(now.hour < 24 && now.minute < 60 && now.second < 61, "{now:?}");
        assert_eq!(now.clock().len(), 8);
        assert_eq!(now.date().len(), 10);
    }

    /// The two constructors stamp themselves and carry the flag they are named for.
    #[test]
    fn the_constructors_stamp_the_line_they_build() {
        let loud = Remark::note("could not send");
        assert!(loud.always);
        assert!(loud.at.year >= 2020, "an unstamped line would sort to the top of the log");
        let quiet = Remark::machinery("ok /theme organon");
        assert!(!quiet.always);
        assert!(quiet.at.year >= 2020);
    }
}

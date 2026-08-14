//! Look-epochs → viewport bands: which rows carry which backdrop (Console Spike
//! Tier 4, Leaf A).
//!
//! The console's backdrop gains **look-epochs**: each `organon console background
//! <name>` closes the current epoch at the then-current end of the buffer and opens a
//! new one (execution plan, ⚡ Sequence amendment, 2026-08-11). The viewport must then
//! render as horizontal **bands** — every visible row shows the look that was active
//! when the text on that row was written. At the live edge the newest look scrolls in
//! from the bottom; scrolled up, history keeps its own styling; nothing is ever
//! restyled after the fact.
//!
//! This module is the arithmetic and nothing else: plain integers in, row ranges out.
//! No egui, no `alacritty_terminal`, no wgpu — so it is `cargo test -p organon-console`
//! in seconds, on any machine, with no GPU and no window.
//!
//! # The coordinate system, and why this one
//!
//! Alacritty's line numbers are **grid-relative and therefore moving targets**:
//! `Line(0)` is the top of the live screen, negative lines are scrollback, and a line
//! of text *loses one* from its index every time another line is emitted. A boundary
//! recorded as a `Line` would slide off its own text within a screenful.
//!
//! So this module works in **absolute line indices**: line 0 is the top row of the very
//! first screen, and every line ever written keeps its number forever. The bridge back
//! to the grid is one addition:
//!
//! ```text
//! abs(grid_line) = screen_top + grid_line          screen_top = dropped + history_size
//! viewport row v  shows  abs = screen_top - display_offset + v
//! ```
//!
//! `screen_top` (the absolute index of `Line(0)`) is **derived every frame, not
//! accumulated**, and that single decision is what makes the rest correct:
//!
//! * **Emission ages a boundary automatically.** Lines that scroll out of the live
//!   screen land in scrollback, so `history_size()` grows by exactly the number
//!   emitted (`grid/mod.rs:252-280`, `scroll_up` → `increase_scroll_limit`), which
//!   moves `screen_top` and leaves every `abs` untouched. Text stays put; the window
//!   moves.
//! * **Scrolling moves the window, not the text.** `display_offset` shifts what the
//!   viewport looks at and nothing else (`grid/mod.rs:163-173`).
//! * **A row resize needs no bookkeeping at all.** Growing the window pulls lines back
//!   *out* of history into the screen — `history_size()` falls by exactly the number
//!   pulled and the cursor moves down by the same amount (`grid/resize.rs:43-70`,
//!   `grow_lines`, the very function R4 flags) — so `screen_top` falls with it and
//!   `abs` is invariant. Shrinking pushes lines the other way through the same
//!   identity (`grid/resize.rs:78-95`, `shrink_lines`). An *accumulated* counter would
//!   have to special-case both directions and would drift the first time it guessed
//!   wrong; a derived one cannot.
//! * **The alternate screen fixes itself.** The alt grid is built with zero scrollback
//!   (`term/mod.rs:415-416`), so while it is up `history_size()` reads 0 — meaningless
//!   as a coordinate, which is exactly why [`bands`] refuses to use it and paints the
//!   alt screen as one band. On the way back, the primary grid's history returns
//!   unchanged and `screen_top` is instantly right again, with nothing to re-baseline.
//!
//! Only one quantity has to be carried by the caller: [`ViewState::dropped`], the count
//! of lines **evicted off the top of scrollback**, which is `0` until the buffer fills
//! (10 000 lines — `term::SCROLLBACK_LINES`) and only then starts to move. Note the
//! pleasing consequence: `dropped` *is* the absolute index of the oldest retained line
//! ([`ViewState::oldest_retained_line`]).
//!
//! # Caller contract — the checklist
//!
//! 1. **Keep one `u64` per session: `dropped`, initialised to 0.** That is the whole
//!    of the persistent state this arithmetic needs. (Item 6 wants one more thing —
//!    the last primary-screen [`ViewState`] and its cursor row — for the alt screen
//!    alone.)
//! 2. **Update it at exactly one site: around `session.pump()`** in `term_view::draw`
//!    (`term_view.rs:154`). That is the only call that advances the parser, so it is
//!    the only place a line can cross the top edge of the live screen:
//!    ```ignore
//!    let before = Snapshot { history: s.term.history_size(),
//!                            display_offset: s.term.grid().display_offset() };
//!    s.pump();
//!    let after  = Snapshot { history: s.term.history_size(),
//!                            display_offset: s.term.grid().display_offset() };
//!    self.dropped = scroll_anchor::advance_dropped(self.dropped, before, after);
//!    ```
//!    `history_size()` needs `use alacritty_terminal::grid::Dimensions` in scope;
//!    `Term` implements it against the **active** grid (`term/mod.rs:1042`).
//! 3. **The bracket must contain the pump and nothing else.** In particular it must not
//!    contain `session.scroll_display(…)` (`term_view.rs:191-194`), because
//!    [`advance_dropped`] reads a `display_offset` *increase* as evidence of emitted
//!    lines, and a user's wheel would then be counted as output. The existing frame
//!    order already separates them.
//! 4. **`session.resize(…)` needs no update** — see above; `screen_top` follows
//!    `history_size()` by construction.
//! 5. **Build [`ViewState`] after the pump**, from `session.size().1`,
//!    `content.display_offset`, `term.history_size()`, `self.dropped`, and
//!    `term.mode().contains(TermMode::ALT_SCREEN)`.
//! 6. **Record a boundary with [`boundary_now`]** when a `background` command
//!    dispatches, and append it with [`push_boundary`] (never bare `Vec::push` — see
//!    the limits below). The cursor row it wants is `content.cursor.point.line.0`, the
//!    same value `term_view` already reads (`term_view.rs:309`) — grid-relative to the
//!    live screen, which is exactly this function's frame of reference. Call it with a
//!    **primary-screen** `ViewState`: while the alt screen is up `history_size()`
//!    reports the alt grid's zero and the coordinates are not comparable, so keep the
//!    last primary `ViewState` (and cursor row) you saw and use those instead. A look
//!    change during `htop` then opens its epoch where the next primary line will
//!    actually be written.
//!    ⚠️ A window **resize** while the alt screen is up still moves the primary — the
//!    inactive grid is resized too (`term/mod.rs:677-678`) — and there is no public
//!    accessor for it, so a boundary taken in that window can be off by the resize
//!    delta. [`push_boundary`] keeps it ordered; nothing can make it exact from out here.
//! 7. **Keep `looks.len() == boundaries.len() + 1`** and index it with [`Band::epoch`]:
//!    epoch 0 is the look before the first boundary. Leaf B's cap evicts an epoch by
//!    **merging it into its older neighbour**, which is exactly "remove
//!    `boundaries[k-1]` and `looks[k]` together" — one splice, no renumbering anywhere
//!    else.
//!
//! # What this cannot do, stated plainly
//!
//! Once scrollback is **full**, `history_size()` is pinned at the cap and only the
//! display pin can still report emitted lines — and the pin itself is pinned at both
//! ends (`grid/mod.rs:267-269`, `min(display_offset + n, max_scroll_limit)`). So in two
//! regimes an evicted line is simply not observable:
//!
//! * at the **live edge** (`display_offset == 0`, the pin does not move at all), and
//! * **parked at the very top** of a full buffer (`display_offset == history == cap`,
//!   the pin is already against its stop).
//!
//! Between those two — a full buffer with the view somewhere in the middle of history —
//! the pin carries the count exactly. Elsewhere `dropped` stalls, so [`advance_dropped`]
//! **under**-counts and never over-counts: the error leaves history looking older than
//! it is rather than younger, and it stops growing the moment the count is observable
//! again. The same gap applies to the one resize that evicts: shrinking rows pushes
//! lines into a full history (`grid/resize.rs:78-95`), and if the pin cannot move those
//! evictions are invisible too — bounded by the resize delta, once per resize.
//!
//! The consequence to know about: while `dropped` lags, `screen_top` lags with it, so a
//! boundary taken *after* a shrink can land below one taken before it. That is why the
//! ledger is appended through [`push_boundary`] rather than `Vec::push` — one clamp,
//! and the ascending order [`bands`] and [`epoch_at`] want is true by construction.
//!
//! Two honest ways out, neither of them this module's to take: raise
//! `term::SCROLLBACK_LINES` (the arithmetic here never mentions the cap, so nothing
//! changes), or count `Grid::scroll_up` at the source, which means a counter inside
//! `alacritty_terminal` or a `Handler` decorator around `Term` in `term.rs`.
//!
//! The other known limit is **column reflow**: changing the width re-wraps rows
//! (`grid/resize.rs`, `grow_columns`/`shrink_columns`), which genuinely renumbers lines,
//! and no absolute index survives that exactly. Row changes are exact; a width change
//! can slide a band edge by the number of wrapped rows above it.
//!
//! Sources: `doc/console_spike_as_built_brief.md` R4 (the scrollback/grid map, the
//! `vrow = point.line.0 + display_offset` render mapping in `term_view.rs:266-271`, and
//! the `session.resize` site), the execution plan's ⚡ Sequence amendment (the Tier 4
//! design cuts), and `alacritty_terminal` 0.26.0 as cited line by line above.

/// One horizontal band of the viewport: an inclusive row range carrying one epoch.
///
/// Rows are **viewport** rows — `0` is the top row drawn, `rows - 1` the bottom — the
/// same `vrow` space `term_view` paints in, so a band is a rect without further
/// arithmetic: `y = rect.top() + first_vrow * cell_h`, height
/// `(last_vrow - first_vrow + 1) * cell_h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub first_vrow: u16,
    pub last_vrow: u16,
    /// Index into the caller's look table; `0` is the look before the first boundary.
    pub epoch: usize,
}

impl Band {
    /// How many rows this band covers (never 0 — [`bands`] emits no empty bands).
    pub fn rows(&self) -> u16 {
        self.last_vrow - self.first_vrow + 1
    }
}

/// One frame of terminal geometry, as plain integers. Every field but `dropped` is read
/// straight off the session; see the caller checklist in the module doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewState {
    /// Screen rows — `session.size().1`, the same bound `term_view` culls against.
    pub rows: u16,
    /// `Grid::display_offset()`: 0 at the live edge, growing as the user scrolls back.
    pub display_offset: usize,
    /// `Dimensions::history_size()` of the **active** grid (0 on the alt screen).
    pub history: usize,
    /// The caller's one counter: lines evicted off the top of scrollback.
    pub dropped: u64,
    /// `TermMode::ALT_SCREEN`. The alt grid has no scrollback, so its geometry says
    /// nothing about absolute lines and [`bands`] short-circuits on it.
    pub alt_screen: bool,
}

impl ViewState {
    /// Absolute index of `Line(0)` — the top row of the **live screen**, not of the
    /// viewport (they differ whenever `display_offset > 0`).
    pub fn screen_top(&self) -> u64 {
        self.dropped.saturating_add(self.history as u64)
    }

    /// Absolute index of the topmost line the terminal still holds. Equal to
    /// `dropped` by construction: what fell off the top is what precedes it.
    pub fn oldest_retained_line(&self) -> u64 {
        self.dropped
    }

    /// Absolute index shown at viewport row 0.
    ///
    /// `display_offset` is clamped to `history` here rather than trusted: alacritty
    /// clamps it the same way (`grid/mod.rs:166`), and a total function is worth more
    /// than an assertion that fires in a paint loop.
    pub fn first_line(&self) -> u64 {
        self.screen_top().saturating_sub(self.display_offset.min(self.history) as u64)
    }

    /// Absolute index shown at viewport row `rows - 1` (== [`Self::first_line`] when
    /// `rows` is 0 or 1).
    pub fn last_line(&self) -> u64 {
        self.first_line() + u64::from(self.rows.saturating_sub(1))
    }

    /// Absolute index of the next line the child will write: one past the bottom of the
    /// live screen. A boundary can legitimately sit here — it simply owns no row yet,
    /// which is what "the new look scrolls in" looks like arithmetically.
    pub fn live_edge(&self) -> u64 {
        self.screen_top() + u64::from(self.rows)
    }
}

/// What to read immediately before and immediately after `session.pump()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub history: usize,
    pub display_offset: usize,
}

/// Which epoch a line belongs to: the number of boundaries at or below it.
///
/// `boundaries` must be ascending (it is append-only and every entry comes from
/// [`boundary_now`], which is monotone). Out-of-order input is not UB and cannot break
/// [`bands`]' partition — it just makes the answer meaningless for that entry.
pub fn epoch_at(boundaries: &[u64], line: u64) -> usize {
    boundaries.partition_point(|b| *b <= line)
}

/// Where a look change opens its new epoch: the line just below the cursor.
///
/// Not the live edge (`screen_top + rows`): on a fresh shell the cursor sits a few rows
/// down and the rest of the screen has never been written, so anchoring at the bottom
/// would hold the new look off-screen for a screenful of output. The rows below the
/// cursor are the future, and the future is what the new look owns — so the change is
/// visible in the same frame it is asked for. Clamped into `[screen_top, live_edge]`.
pub fn boundary_now(state: ViewState, cursor_line: i32) -> u64 {
    let row = cursor_line.saturating_add(1).clamp(0, i32::from(state.rows)) as u64;
    state.screen_top() + row
}

/// Append a boundary to the ledger, keeping it ascending. Returns what was recorded.
///
/// With an exact `dropped` this only ever records `line` unchanged: [`boundary_now`] is
/// monotone under output and *invariant* under a row resize, because `screen_top` and
/// the cursor move by the same amount in opposite directions (`grid/resize.rs:43-70`
/// moves both). The clamp exists for the regime where `dropped` cannot see an eviction
/// (module doc, "What this cannot do"): there `screen_top` lags, and a shrink — which
/// pulls the cursor up while the true live edge moves down — can compute a boundary
/// below the previous one. Recording it as-is would put two epochs out of order
/// permanently; recording `max` says the honest thing instead, that the new epoch
/// begins no earlier than the one before it.
pub fn push_boundary(ledger: &mut Vec<u64>, line: u64) -> u64 {
    let recorded = match ledger.last() {
        Some(prev) => line.max(*prev),
        None => line,
    };
    ledger.push(recorded);
    recorded
}

/// Advance the caller's `dropped` counter across one `pump()`. See checklist items 2–3.
///
/// The rule in one line: **lines emitted = the larger of what history gained and what
/// the display pin moved; whatever history could not absorb was dropped.**
///
/// * Live, buffer not full — history gains exactly the emitted lines, the pin sits at 0.
/// * Scrolled back — alacritty *also* advances `display_offset` to keep the same text
///   under the viewport (`grid/mod.rs:267-269`), so both move by the same count and
///   `max` (never a sum) reads it once.
/// * Buffer full, scrolled into the middle of history — history is pinned at the cap
///   and the pin alone carries the count; the difference is what fell off the top.
/// * Buffer full, and the pin cannot move either — at the live edge, or parked against
///   the top of the buffer. Returns unchanged; this is the blind spot named in the
///   module doc, and holding still is the whole point: an unobservable eviction must
///   not become an invented one.
///
/// A **negative** history delta means something other than output moved the grid — a
/// resize slipped into the bracket, or the child swapped to the alt screen mid-pump
/// (`term/mod.rs:714-732`). Neither drops scrollback lines, so the counter holds still
/// rather than inventing a number from a delta it cannot interpret.
pub fn advance_dropped(dropped: u64, before: Snapshot, after: Snapshot) -> u64 {
    let gained = after.history as i64 - before.history as i64;
    let pinned = after.display_offset as i64 - before.display_offset as i64;
    if gained < 0 {
        return dropped;
    }
    let emitted = gained.max(pinned.max(0));
    dropped.saturating_add((emitted - gained) as u64)
}

/// The viewport, cut into look-epoch bands, top to bottom.
///
/// Guarantees, each of them a test below:
///
/// * the bands **partition** `[0, rows)` — contiguous, no gap, no overlap, none empty;
/// * `first_vrow` and `epoch` are both **strictly increasing** down the list;
/// * no boundaries (or none in range) → exactly one band of the youngest epoch present;
/// * a boundary **older than the retained history** is not lost: the rows it governs
///   still report its epoch, because every retained line is at or after it. Several
///   such boundaries collapse to the newest of them, which is the truth — the older
///   epochs own no retained line;
/// * a boundary **below the viewport** (the user has scrolled up past it) contributes
///   nothing: the visible rows report older epochs only;
/// * the **alternate screen is always exactly one band** of the newest epoch. It has no
///   scrollback to band (`term/mod.rs:415-416`), and its rows are a live application's
///   canvas rather than a transcript.
pub fn bands(boundaries: &[u64], state: ViewState) -> Vec<Band> {
    let rows = state.rows;
    if rows == 0 {
        return Vec::new();
    }
    if state.alt_screen {
        return vec![Band { first_vrow: 0, last_vrow: rows - 1, epoch: boundaries.len() }];
    }

    let first = state.first_line();
    let last = state.last_line();

    let mut out: Vec<Band> = Vec::new();
    let mut start: u16 = 0;
    // Everything at or below the top row belongs to one epoch — that is the implicit
    // clamp for boundaries older than the retained window; they need no special case.
    let mut epoch = epoch_at(boundaries, first);
    let mut i = epoch;
    while i < boundaries.len() {
        let b = boundaries[i];
        if b > last {
            break; // and everything after it, the slice being ascending.
        }
        // Saturating and clamped so that a duplicate (two look changes with no output
        // between them) or an out-of-order entry can never produce an empty band, an
        // inverted range, or an underflow — it just folds into the band below, carrying
        // the newer epoch, which is what a zero-line epoch means.
        let v = b.saturating_sub(first).min(u64::from(rows - 1)) as u16;
        if v > start {
            out.push(Band { first_vrow: start, last_vrow: v - 1, epoch });
            start = v;
        }
        i += 1;
        epoch = i;
    }
    out.push(Band { first_vrow: start, last_vrow: rows - 1, epoch });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scrollback cap this module is used under (`term::SCROLLBACK_LINES`, private
    /// there). Nothing in the module reads it — the arithmetic is cap-agnostic on
    /// purpose — so the sweeps below use small caps to reach saturation cheaply and
    /// this one to prove the real number behaves the same.
    const CAP: usize = 10_000;

    /// A model of the exact `alacritty_terminal` 0.26 rules this module is written
    /// against, carrying the truth (`screen_top`) the module has to reconstruct from
    /// observables. Every transition below cites the function it mirrors; if alacritty
    /// changes, this is the file that should fail first.
    struct Vt {
        cap: usize,
        rows: u16,
        alt: bool,
        /// The primary grid — the only one with scrollback.
        cursor: u16,
        history: usize,
        display_offset: usize,
        /// Truth: lines evicted off the top of the primary's scrollback.
        dropped: u64,
        /// Truth: absolute index of the primary's `Line(0)`.
        screen_top: u64,
        /// The alt grid: same rows, `max_scroll_limit` 0, so no history and no offset
        /// (`term/mod.rs:415-416`). Only its cursor is state.
        alt_cursor: u16,
    }

    impl Vt {
        fn new(rows: u16, cap: usize) -> Self {
            Vt {
                cap,
                rows,
                alt: false,
                cursor: 0,
                history: 0,
                display_offset: 0,
                dropped: 0,
                screen_top: 0,
                alt_cursor: 0,
            }
        }

        /// `n` lines of output scroll off the top of the ACTIVE grid: `grid/mod.rs:252-280`
        /// (`scroll_up`) plus `:175-180` (`increase_scroll_limit`, which caps the growth).
        /// On the alt grid that cap is 0, so nothing is retained.
        fn emit(&mut self, n: usize) {
            if self.alt {
                if n > 0 {
                    self.alt_cursor = self.rows - 1;
                }
                return;
            }
            let gained = n.min(self.cap - self.history);
            if self.display_offset != 0 {
                self.display_offset = (self.display_offset + n).min(self.cap);
            }
            self.history += gained;
            self.dropped += (n - gained) as u64;
            self.screen_top += n as u64;
            if n > 0 {
                self.cursor = self.rows - 1;
            }
        }

        /// `Scroll::Delta`, positive = up into history: `grid/mod.rs:163-173`. Clamped
        /// to `history_size()`, which is 0 on the alt screen — there is nowhere to go.
        fn scroll(&mut self, delta: i32) {
            if self.alt {
                return;
            }
            let next = (self.display_offset as i64 + delta as i64).max(0) as usize;
            self.display_offset = next.min(self.history);
        }

        /// `grid/resize.rs:43-70` (`grow_lines`), applied to BOTH grids — `Term::resize`
        /// resizes the inactive one too (`term/mod.rs:677-678`), which is why the primary
        /// keeps moving while `htop` is up. Lines come back out of history and the cursor
        /// follows them down; with no history to draw on (always, for the alt grid) blank
        /// rows are added at the bottom and nothing moves — that function's
        /// `scroll_up`/`decrease_scroll_limit` pair cancels exactly.
        fn grow_rows(&mut self, k: u16) {
            let from_history = self.history.min(k as usize);
            self.history -= from_history;
            self.screen_top -= from_history as u64;
            self.display_offset = self.display_offset.saturating_sub(k as usize);
            self.cursor += from_history as u16;
            self.rows += k;
        }

        /// `grid/resize.rs:78-95` (`shrink_lines`): the cursor is kept on screen by
        /// pushing history "out the top", which is a `scroll_up` and therefore evicts
        /// when the buffer is full. The alt grid, having no scrollback, simply discards.
        fn shrink_rows(&mut self, k: u16) {
            let target = self.rows - k;
            self.alt_cursor = self.alt_cursor.min(target - 1);

            let required = (self.cursor as usize + 1).saturating_sub(target as usize);
            if required > 0 {
                let gained = required.min(self.cap - self.history);
                if self.display_offset != 0 {
                    self.display_offset = (self.display_offset + required).min(self.cap);
                }
                self.history += gained;
                self.dropped += (required - gained) as u64;
                self.screen_top += required as u64;
                self.cursor = self.cursor.min(target - 1);
            }
            self.rows = target;
            self.display_offset = self.display_offset.min(self.history);
        }

        /// `term/mod.rs:714-732` (`swap_alt`): the grids trade places. The primary's
        /// scrollback is untouched while it waits.
        fn toggle_alt(&mut self) {
            self.alt = !self.alt;
            if self.alt {
                self.alt_cursor = 0;
            }
        }

        /// What `Dimensions::history_size()` and `Grid::display_offset()` report: the
        /// ACTIVE grid, always (`term/mod.rs:1042`).
        fn snapshot(&self) -> Snapshot {
            if self.alt {
                Snapshot { history: 0, display_offset: 0 }
            } else {
                Snapshot { history: self.history, display_offset: self.display_offset }
            }
        }

        /// What the caller would hand the module, with its own (possibly lagging)
        /// `dropped` estimate.
        fn view(&self, dropped: u64) -> ViewState {
            let seen = self.snapshot();
            ViewState {
                rows: self.rows,
                display_offset: seen.display_offset,
                history: seen.history,
                dropped,
                alt_screen: self.alt,
            }
        }
    }

    /// Deterministic, dependency-free, and good enough to interleave operations —
    /// there is no new dev-dependency in this crate for a sweep.
    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Lcg(seed.wrapping_mul(2862933555777941757).wrapping_add(3037000493))
        }
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    fn assert_partition(out: &[Band], rows: u16) {
        if rows == 0 {
            assert!(out.is_empty(), "no rows, no bands: {out:?}");
            return;
        }
        assert!(!out.is_empty(), "{rows} rows must be covered by at least one band");
        assert_eq!(out[0].first_vrow, 0, "the first band must start at the top: {out:?}");
        assert_eq!(
            out.last().unwrap().last_vrow,
            rows - 1,
            "the last band must reach the bottom row: {out:?}"
        );
        for b in out {
            assert!(b.first_vrow <= b.last_vrow, "empty or inverted band in {out:?}");
        }
        for w in out.windows(2) {
            assert_eq!(w[1].first_vrow, w[0].last_vrow + 1, "gap or overlap in {out:?}");
            assert!(w[1].epoch > w[0].epoch, "epochs must increase downward: {out:?}");
        }
    }

    /// The definition, spelled out one row at a time — the reference [`bands`] is
    /// checked against everywhere below.
    fn per_row(boundaries: &[u64], st: ViewState) -> Vec<usize> {
        if st.alt_screen {
            return vec![boundaries.len(); st.rows as usize];
        }
        (0..u64::from(st.rows)).map(|v| epoch_at(boundaries, st.first_line() + v)).collect()
    }

    fn expand(out: &[Band]) -> Vec<usize> {
        let mut rows = Vec::new();
        for b in out {
            for _ in b.first_vrow..=b.last_vrow {
                rows.push(b.epoch);
            }
        }
        rows
    }

    fn live(rows: u16, history: usize, dropped: u64) -> ViewState {
        ViewState { rows, display_offset: 0, history, dropped, alt_screen: false }
    }

    #[test]
    fn no_boundaries_is_one_band_of_epoch_zero() {
        let st = live(24, 0, 0);
        assert_eq!(bands(&[], st), vec![Band { first_vrow: 0, last_vrow: 23, epoch: 0 }]);
        assert_eq!(bands(&[], st)[0].rows(), 24);
    }

    #[test]
    fn zero_rows_yields_no_bands() {
        let st = live(0, 500, 0);
        assert!(bands(&[10, 20], st).is_empty());
        assert!(bands(&[10, 20], ViewState { alt_screen: true, ..st }).is_empty());
    }

    /// The plain case: two changes inside the visible window, three bands.
    #[test]
    fn a_boundary_inside_the_viewport_splits_it() {
        // 100 lines have scrolled off; the viewport shows absolute 100..110.
        let st = live(10, 100, 0);
        assert_eq!(st.first_line(), 100);
        assert_eq!(st.last_line(), 109);

        let out = bands(&[103, 107], st);
        assert_eq!(
            out,
            vec![
                Band { first_vrow: 0, last_vrow: 2, epoch: 0 },
                Band { first_vrow: 3, last_vrow: 6, epoch: 1 },
                Band { first_vrow: 7, last_vrow: 9, epoch: 2 },
            ]
        );
        assert_partition(&out, st.rows);
        assert_eq!(expand(&out), per_row(&[103, 107], st));
    }

    /// The Tier 4 beat, in arithmetic: ask for a new look, and it arrives at the cursor
    /// and grows upward as output pushes the old text into history.
    #[test]
    fn the_new_look_scrolls_in_from_the_bottom() {
        let mut vt = Vt::new(10, CAP);
        vt.emit(4); // four lines of output; cursor at the bottom row
        let st = vt.view(vt.dropped);
        let b = boundary_now(st, 3);
        assert_eq!(b, st.screen_top() + 4, "the epoch opens just below the cursor");

        // Nothing has scrolled yet: the change owns the untouched rows below the cursor.
        let out = bands(&[b], st);
        assert_eq!(out, vec![
            Band { first_vrow: 0, last_vrow: 3, epoch: 0 },
            Band { first_vrow: 4, last_vrow: 9, epoch: 1 },
        ]);

        // Three screenfuls later the old look has scrolled clean off the top.
        vt.emit(30);
        let st = vt.view(vt.dropped);
        assert_eq!(bands(&[b], st), vec![Band { first_vrow: 0, last_vrow: 9, epoch: 1 }]);
    }

    /// Scrolling moves the window, not the text: the same boundary, further down the
    /// screen, one row per line scrolled — and history above it keeps epoch 0.
    #[test]
    fn scrolling_back_moves_the_edge_down_not_the_text() {
        let mut vt = Vt::new(10, CAP);
        vt.emit(40);
        let b = boundary_now(vt.view(vt.dropped), 9);
        vt.emit(10);

        let at_live = bands(&[b], vt.view(vt.dropped));
        assert_eq!(at_live, vec![Band { first_vrow: 0, last_vrow: 9, epoch: 1 }]);

        vt.scroll(4);
        let scrolled = bands(&[b], vt.view(vt.dropped));
        assert_eq!(scrolled, vec![
            Band { first_vrow: 0, last_vrow: 3, epoch: 0 },
            Band { first_vrow: 4, last_vrow: 9, epoch: 1 },
        ]);

        // Scroll past it entirely: older epochs only, no trace of the change.
        vt.scroll(20);
        let far = bands(&[b], vt.view(vt.dropped));
        assert_eq!(far, vec![Band { first_vrow: 0, last_vrow: 9, epoch: 0 }]);
        assert_partition(&far, 10);
    }

    /// A boundary older than everything the terminal still holds must not vanish and
    /// must not renumber: its epoch is what the retained rows carry.
    #[test]
    fn boundaries_older_than_the_retained_history_clamp() {
        // A tiny scrollback that has already wrapped: 5 000 lines gone, 50 retained.
        let st = ViewState {
            rows: 10,
            display_offset: 0,
            history: 50,
            dropped: 5_000,
            alt_screen: false,
        };
        assert_eq!(st.oldest_retained_line(), 5_000);

        // Three boundaries, all long gone off the top.
        let out = bands(&[7, 900, 4_000], st);
        assert_eq!(
            out,
            vec![Band { first_vrow: 0, last_vrow: 9, epoch: 3 }],
            "every retained line is after all three, so the newest epoch owns the screen"
        );
        assert_partition(&out, st.rows);

        // One of them still in range proves the clamp is not "drop them all".
        let out = bands(&[7, 900, 4_000, st.first_line() + 6], st);
        assert_eq!(out, vec![
            Band { first_vrow: 0, last_vrow: 5, epoch: 3 },
            Band { first_vrow: 6, last_vrow: 9, epoch: 4 },
        ]);
    }

    #[test]
    fn alt_screen_is_always_exactly_one_band_of_the_newest_epoch() {
        // Deliberately nonsense scrollback geometry: the alt screen has none, and the
        // answer must not depend on any of it.
        for (history, offset, dropped) in [(0, 0, 0u64), (900, 400, 12), (0, 77, 4_000)] {
            let st = ViewState { rows: 30, display_offset: offset, history, dropped, alt_screen: true };
            let out = bands(&[3, 40, 41, 9_000], st);
            assert_eq!(out, vec![Band { first_vrow: 0, last_vrow: 29, epoch: 4 }]);
            assert_partition(&out, st.rows);
        }
    }

    /// Two look changes with no output between them: the middle epoch owns no line, and
    /// an epoch with no lines gets no band rather than an empty one.
    #[test]
    fn duplicate_boundaries_do_not_produce_empty_bands() {
        let st = live(8, 100, 0);
        let out = bands(&[104, 104, 104], st);
        assert_eq!(out, vec![
            Band { first_vrow: 0, last_vrow: 3, epoch: 0 },
            Band { first_vrow: 4, last_vrow: 7, epoch: 3 },
        ]);
        assert_partition(&out, st.rows);
        assert_eq!(expand(&out), per_row(&[104, 104, 104], st));
    }

    /// The ledger is append-only and [`boundary_now`] is monotone, so this cannot
    /// happen — but a partition that depends on a precondition is a partition that
    /// breaks in the paint loop. It must stay total.
    #[test]
    fn unsorted_boundaries_still_partition() {
        let st = live(12, 200, 0);
        for set in [
            vec![208u64, 203, 205],
            vec![205, 205, 199, 500, 201],
            vec![u64::MAX, 0, 206],
        ] {
            assert_partition(&bands(&set, st), st.rows);
        }
    }

    #[test]
    fn epoch_at_counts_boundaries_at_or_below_the_line() {
        let b = [10u64, 20, 20, 30];
        assert_eq!(epoch_at(&b, 0), 0);
        assert_eq!(epoch_at(&b, 9), 0);
        assert_eq!(epoch_at(&b, 10), 1, "the boundary line is the FIRST line of its epoch");
        assert_eq!(epoch_at(&b, 19), 1);
        assert_eq!(epoch_at(&b, 20), 3, "a zero-line epoch is skipped, not smeared");
        assert_eq!(epoch_at(&b, 999), 4);
        assert_eq!(epoch_at(&[], 42), 0);
    }

    #[test]
    fn boundary_now_sits_below_the_cursor_and_never_past_the_live_edge() {
        let st = live(24, 300, 0);
        assert_eq!(boundary_now(st, 0), st.screen_top() + 1);
        assert_eq!(boundary_now(st, 5), st.screen_top() + 6);
        assert_eq!(boundary_now(st, 23), st.live_edge(), "the bottom row means: next line");
        assert_eq!(boundary_now(st, 99), st.live_edge(), "clamped, never past the edge");
        assert_eq!(boundary_now(st, -7), st.screen_top(), "and never above the screen");
        // Scrolling does not move where a new epoch opens — the live edge is elsewhere.
        let scrolled = ViewState { display_offset: 120, ..st };
        assert_eq!(boundary_now(scrolled, 5), boundary_now(st, 5));
    }

    /// A boundary taken at the live edge survives a resize unchanged, because both
    /// `screen_top` and the cursor move by the same amount in opposite directions.
    #[test]
    fn a_boundary_is_invariant_across_row_resizes() {
        for cap in [CAP, 64] {
            let mut vt = Vt::new(30, cap);
            vt.emit(200);
            let before = boundary_now(vt.view(vt.dropped), vt.cursor as i32);

            vt.grow_rows(7);
            assert_eq!(boundary_now(vt.view(vt.dropped), vt.cursor as i32), before, "grow");
            vt.shrink_rows(12);
            assert_eq!(boundary_now(vt.view(vt.dropped), vt.cursor as i32), before, "shrink");
        }
    }

    /// The identity the whole design rests on. If this ever fails, `screen_top` cannot
    /// be derived and the module needs a different coordinate, not a patch.
    #[test]
    fn screen_top_is_dropped_plus_history_under_every_operation() {
        let mut rng = Lcg::new(7);
        for cap in [16usize, 64, 1_000, CAP] {
            let mut vt = Vt::new(24, cap);
            for _ in 0..400 {
                match rng.below(5) {
                    0 | 1 => vt.emit(rng.below(97) as usize),
                    2 => vt.scroll(rng.below(60) as i32 - 30),
                    3 => {
                        let k = 1 + rng.below(6) as u16;
                        vt.grow_rows(k);
                    }
                    _ => {
                        let k = (1 + rng.below(6) as u16).min(vt.rows.saturating_sub(2));
                        if k > 0 {
                            vt.shrink_rows(k);
                        }
                    }
                }
                assert_eq!(
                    vt.screen_top,
                    vt.dropped + vt.history as u64,
                    "cap {cap}: screen_top must stay derivable from what the grid reports"
                );
                assert!(vt.display_offset <= vt.history, "display_offset outran history");
            }
        }
    }

    /// The counter rule, against the model's truth: exact until the buffer is full, and
    /// after that exact whenever the display pin is free to move. Where it is not, the
    /// counter stalls — never over-counts — and the gap stops growing again the moment
    /// the pin comes free.
    #[test]
    fn advance_dropped_is_exact_while_the_grid_can_show_an_eviction() {
        // 1. Unsaturated, live: history carries the whole count, nothing drops.
        let mut vt = Vt::new(20, 100);
        let (b, mut est) = (vt.snapshot(), 0u64);
        vt.emit(40);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!((est, vt.dropped), (0, 0));

        // 2. Unsaturated, scrolled back: the display pin moves too — max, not a sum.
        vt.scroll(10);
        let b = vt.snapshot();
        vt.emit(30);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!((est, vt.dropped), (0, 0), "a pinned view is not an eviction");

        // 3. The frame that crosses the cap: part absorbed, the rest dropped.
        let b = vt.snapshot();
        vt.emit(60);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!(est, vt.dropped, "the crossing frame must split correctly");
        assert!(vt.dropped > 0 && vt.history == vt.cap);

        // 4. Full buffer, viewing the middle of history: the pin alone carries it.
        vt.scroll(-40);
        assert!(vt.display_offset > 0 && vt.display_offset < vt.cap, "the pin must be free");
        let b = vt.snapshot();
        vt.emit(25);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!(est, vt.dropped);

        // 5. Full buffer at the live edge: nothing observable moves. The first blind
        //    regime — the counter stalls rather than guessing.
        vt.scroll(-10_000);
        assert_eq!(vt.display_offset, 0);
        let b = vt.snapshot();
        vt.emit(50);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!(vt.dropped - est, 50, "an under-count of exactly the invisible lines");

        // 6. Full buffer, parked against the top: the pin is already at its stop, so
        //    this is blind for the same reason. The second regime, and the last.
        vt.scroll(10_000);
        assert_eq!(vt.display_offset, vt.cap);
        let b = vt.snapshot();
        vt.emit(20);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!(vt.dropped - est, 70, "the gap grew by the 20, and by nothing else");

        // 7. Come off the stop and the count is exact again — the gap stops growing.
        vt.scroll(-30);
        let b = vt.snapshot();
        vt.emit(15);
        est = advance_dropped(est, b, vt.snapshot());
        assert_eq!(vt.dropped - est, 70, "recovery: no new error once the pin is free");
    }

    /// The ledger stays ascending even when `dropped` is lagging — which is the only
    /// way [`boundary_now`] can go backwards (a shrink pulls the cursor up while the
    /// unobserved live edge moves down).
    #[test]
    fn push_boundary_keeps_the_ledger_ascending() {
        let mut ledger = Vec::new();
        assert_eq!(push_boundary(&mut ledger, 140), 140);
        assert_eq!(push_boundary(&mut ledger, 148), 148);
        assert_eq!(push_boundary(&mut ledger, 143), 148, "a late boundary cannot go earlier");
        assert_eq!(push_boundary(&mut ledger, 900), 900);
        assert_eq!(ledger, vec![140, 148, 148, 900]);
        assert!(ledger.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn alt_screen_transitions_do_not_move_the_counter() {
        let mut vt = Vt::new(24, CAP);
        vt.emit(500);
        vt.scroll(30);

        let b = vt.snapshot();
        vt.toggle_alt(); // history collapses to 0 — a negative delta, not an eviction
        assert_eq!(advance_dropped(9, b, vt.snapshot()), 9);

        let b = vt.snapshot();
        vt.emit(300); // a full-screen app churning: nothing reaches scrollback
        vt.toggle_alt();
        assert_eq!(advance_dropped(9, b, vt.snapshot()), 9, "the primary's history returns");
        assert_eq!(vt.history, 500, "and returns intact");
        assert_eq!(vt.display_offset, 30);
    }

    /// The sweep: output, look changes, scrolling, resizes and alt-screen toggles
    /// interleaved, with the invariants checked after **every** step — across the
    /// saturation point in three of the four runs.
    #[test]
    fn interleaved_sessions_keep_every_invariant() {
        for (seed, cap) in [(1u64, 32usize), (2, 128), (3, 1_500), (4, CAP)] {
            let mut rng = Lcg::new(seed);
            let mut vt = Vt::new(1 + rng.below(40) as u16 + 4, cap);
            let mut dropped = 0u64; // the caller's counter, maintained by the contract
            let mut ledger: Vec<u64> = Vec::new();
            // Checklist item 6: the last primary view, for boundaries taken under alt.
            let mut last_primary = vt.view(dropped);
            let mut last_primary_cursor = vt.cursor;

            for step in 0..500 {
                match rng.below(8) {
                    0..=2 => {
                        // Output, with the counter updated exactly as the contract says.
                        let before = vt.snapshot();
                        vt.emit(rng.below(220) as usize);
                        dropped = advance_dropped(dropped, before, vt.snapshot());
                    }
                    3 => vt.scroll(rng.below(120) as i32 - 50),
                    4 => {
                        let k = 1 + rng.below(5) as u16;
                        vt.grow_rows(k);
                    }
                    5 => {
                        let k = (1 + rng.below(5) as u16).min(vt.rows.saturating_sub(3));
                        if k > 0 {
                            vt.shrink_rows(k);
                        }
                    }
                    6 => {
                        // Checklist item 6, exactly as written: under the alt screen the
                        // boundary comes from the last primary view, not from the alt
                        // grid's incomparable coordinates.
                        let (st, cursor) = if vt.alt {
                            (last_primary, last_primary_cursor)
                        } else {
                            (vt.view(dropped), vt.cursor)
                        };
                        push_boundary(&mut ledger, boundary_now(st, i32::from(cursor)));
                    }
                    _ => {
                        let before = vt.snapshot();
                        vt.toggle_alt();
                        dropped = advance_dropped(dropped, before, vt.snapshot());
                    }
                }

                if !vt.alt {
                    last_primary = vt.view(dropped);
                    last_primary_cursor = vt.cursor;
                }
                let st = vt.view(dropped);
                let out = bands(&ledger, st);

                assert_partition(&out, st.rows);
                assert_eq!(
                    expand(&out),
                    per_row(&ledger, st),
                    "cap {cap} step {step}: bands disagree with the per-row definition"
                );
                assert!(
                    ledger.windows(2).all(|w| w[0] <= w[1]),
                    "cap {cap} step {step}: the ledger must stay ascending: {ledger:?}"
                );
                assert!(
                    dropped <= vt.dropped,
                    "cap {cap} step {step}: the counter must never over-count"
                );
                if !vt.alt {
                    assert_eq!(
                        vt.view(vt.dropped).screen_top(),
                        vt.screen_top,
                        "cap {cap} step {step}: an exact counter must reconstruct screen_top"
                    );
                }
                assert!(
                    st.first_line() >= st.oldest_retained_line(),
                    "cap {cap} step {step}: the viewport must sit inside retained history"
                );
            }
            assert!(!ledger.is_empty(), "seed {seed}: the sweep never recorded a boundary");
        }
    }

    /// Rows change constantly (window drags, the Tier 3 strip appearing and hiding);
    /// the partition may not depend on the number.
    #[test]
    fn every_row_count_partitions() {
        let boundaries: Vec<u64> = vec![0, 1, 7, 8, 9, 64, 900, 901, 5_000];
        for rows in [0u16, 1, 2, 3, 5, 24, 41, 200, 1_000] {
            for (history, offset, dropped) in
                [(0usize, 0usize, 0u64), (900, 0, 0), (900, 450, 0), (10_000, 10_000, 3_333)]
            {
                let st = ViewState {
                    rows,
                    display_offset: offset,
                    history,
                    dropped,
                    alt_screen: false,
                };
                let out = bands(&boundaries, st);
                assert_partition(&out, rows);
                assert_eq!(expand(&out), per_row(&boundaries, st), "rows {rows}");
            }
        }
    }
}

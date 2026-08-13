//! Blocks → viewport row ranges: which rows a reserved run of lines occupies right now
//! (Console Spike Tier 5).
//!
//! The console is gaining **blocks**: a contiguous run of N lines reserved inside the
//! scrollback, into which a GPU-rendered texture is later painted — an Organon scene, a
//! panel of interactive controls, whatever the block's owner draws. A block is pinned to
//! *its own lines*. It scrolls with the transcript, it is clipped at the viewport edges,
//! and it disappears when its lines do.
//!
//! This module is the arithmetic and nothing else: plain integers in, viewport row ranges
//! out. No egui, no `alacritty_terminal`, no wgpu — so it is `cargo test -p organon-console`
//! in seconds, on any machine, with no GPU and no window.
//!
//! # It borrows its coordinates, it does not invent them
//!
//! Everything here works in the **absolute line indices** established by
//! [`scroll_anchor`](crate::scroll_anchor) — line 0 is the top row of the very first
//! screen and every line ever written keeps its number forever — and takes that module's
//! [`ViewState`] verbatim rather than growing a second description of where the viewport
//! is. Two answers to "which text is on screen" is exactly the drift this codebase keeps
//! paying for; read `scroll_anchor`'s module doc for the derivation, the caller contract
//! for `dropped`, and the two regimes where `dropped` is allowed to lag.
//!
//! The consequence worth stating: a block is **not** a rectangle that gets moved. It is a
//! pair of numbers that never change (`first_abs`, `rows`), and every frame asks where —
//! if anywhere — those lines currently are. Nothing has to be updated when the user
//! scrolls, when output arrives, or when the window is resized, for the same reason
//! `scroll_anchor` needs no accumulator: the text stays put and the window moves.
//!
//! # Why a band carries `block_row`
//!
//! [`BlockBand`] echoes [`scroll_anchor::Band`](crate::scroll_anchor::Band) — an inclusive
//! viewport row range — and adds one field, [`BlockBand::block_row`]. Extents alone are
//! not enough to paint a block: when a block is half-scrolled off the top, the painter
//! knows *where* to draw but not *what*, and drawing the whole texture into the surviving
//! rows would squash it and slide it as the user scrolls. `block_row` is the block's own
//! row index — measured from `first_abs`, the block's original origin — of the line drawn
//! at `first_vrow`. The texture slice is therefore
//! `block_row .. block_row + band.rows()`, in a coordinate that a texture rendered once,
//! for the whole block, can be sampled in directly. It is measured from `first_abs` rather
//! than from the block's *surviving* top precisely so that erosion (below) does not
//! silently re-index a texture that was rendered before the erosion happened.
//!
//! # Erosion, and the one thing the clip does not do for you
//!
//! Scrollback is capped (`term::SCROLLBACK_LINES`, 10 000). When the cap is reached lines
//! are evicted off the top, and a block does not vanish atomically — it is overwritten one
//! row at a time from the top. [`ViewState::oldest_retained_line`] is the authority (it is
//! exactly `dropped`).
//!
//! For **visibility** this needs no special case, and it is worth knowing why rather than
//! trusting the code: every viewport row satisfies `abs >= dropped` by construction
//! (`first_line()` is `screen_top - min(display_offset, history)`, and `screen_top` is
//! `dropped + history`), so an eroded row can never be a visible row. The clip against the
//! viewport already does the whole job; the explicit clamp in [`visible_blocks`] is
//! belt-and-braces against an inconsistent `ViewState`, and
//! [`erosion_is_already_implied_by_the_viewport_clip`](self) pins the equivalence.
//!
//! For **reaping** it very much does need saying, and that is the load-bearing half: a
//! block owns a GPU texture, and a block whose last line has been evicted will never be
//! visible again, so nothing else will ever tell the caller to free it. That is what
//! [`Block::retained_rows`] is for — `0` means gone, drop it. It takes the oldest retained
//! line as a bare `u64` rather than a `ViewState` on purpose: reaping must keep working
//! while the alt screen is up, where the grid's geometry says nothing but the caller's own
//! `dropped` counter is still valid.
//!
//! # Overlap is a bug at the insertion site, not at the paint site
//!
//! Two blocks cannot honestly claim the same lines: a block reserves its lines by emitting
//! them, so an overlap is a bookkeeping error upstream. This module refuses to make that
//! error expensive:
//!
//! * [`visible_blocks`] is **total** and never panics. Each block's arithmetic is
//!   independent of every other's, so each answer is still individually correct; bands are
//!   emitted in **input order**, which makes the input slice the caller's z-order — an
//!   overlap paints later-block-wins, the ordinary meaning of drawing two rects in
//!   sequence. (A caller wanting the screen top-to-bottom sorts by `first_vrow`.)
//! * [`overlapping`] is the check, offered separately, to be run **once when a block is
//!   inserted** — or behind a `debug_assert!` — never every frame. `scroll_anchor` makes
//!   the same call for the same reason: a total function is worth more than an assertion
//!   that fires in a paint loop.
//!
//! A block with `rows == 0` reserves nothing, so it is invisible, never overlaps, and is
//! never reapable — it is not an error, just a block that is not there yet.
//!
//! # The alternate screen emits nothing
//!
//! The alt grid is built with zero scrollback, so while it is up `history_size()` reads 0
//! and is meaningless as a coordinate — [`scroll_anchor::bands`](crate::scroll_anchor::bands)
//! refuses to use it, and so does this. A full-screen application's canvas is not a
//! transcript and holds no reserved lines; blocks reappear, unchanged, when the primary
//! grid comes back, because nothing about them was ever stored in the grid.

use crate::scroll_anchor::ViewState;

/// A contiguous run of lines reserved in the scrollback for something other than text.
///
/// Both fields are set once, when the block is created, and never move again. What moves
/// is the viewport — see the module doc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    /// Absolute index of the block's first reserved line, in `scroll_anchor` coordinates.
    pub first_abs: u64,
    /// How many lines it reserves. `0` is legal and means "occupies nothing".
    pub rows: u16,
}

impl Block {
    /// One past the block's last reserved line — the absolute index of the first line
    /// *after* it. Saturating, so an absurd `first_abs` cannot wrap into a live range.
    pub fn end_abs(&self) -> u64 {
        self.first_abs.saturating_add(u64::from(self.rows))
    }

    /// How many rows have been evicted off the block's **top**, given the oldest line the
    /// terminal still holds ([`ViewState::oldest_retained_line`], i.e. `dropped`).
    ///
    /// Never more than `rows`: a block cannot lose more than it had.
    pub fn eroded_rows(&self, oldest_retained: u64) -> u16 {
        oldest_retained.saturating_sub(self.first_abs).min(u64::from(self.rows)) as u16
    }

    /// How many of the block's rows the terminal still holds. **`0` is the reap signal**:
    /// the block can never be visible again, so its texture should be freed.
    ///
    /// Erosion is strictly top-down — the bottom of a block is never eaten — so this is
    /// simply `rows - eroded_rows`.
    pub fn retained_rows(&self, oldest_retained: u64) -> u16 {
        self.rows - self.eroded_rows(oldest_retained)
    }
}

/// Where one block is on screen this frame, and which slice of it is showing.
///
/// Rows are **viewport** rows — `0` is the top row drawn, `rows - 1` the bottom — the same
/// `vrow` space `term_view` paints in, so this is a rect without further arithmetic:
/// `y = rect.top() + first_vrow * cell_h`, height `rows() * cell_h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockBand {
    /// Index into the slice handed to [`visible_blocks`]. Invisible blocks emit no band,
    /// so position in the output says nothing — this is the only way back to the block.
    pub block: usize,
    pub first_vrow: u16,
    pub last_vrow: u16,
    /// The block's **own** row index, counted from `first_abs`, of the line drawn at
    /// `first_vrow`. Sample the texture over `block_row .. block_row + rows()`.
    ///
    /// `0` whenever the block's top is on screen; positive when it is clipped above the
    /// viewport, or eroded, or both. See the module doc for why it is anchored to
    /// `first_abs` rather than to the surviving top.
    pub block_row: u16,
}

impl BlockBand {
    /// How many rows this band covers (never 0 — [`visible_blocks`] emits no empty bands).
    pub fn rows(&self) -> u16 {
        self.last_vrow - self.first_vrow + 1
    }
}

/// The visible part of every block, in input order.
///
/// Guarantees, each of them a test below:
///
/// * every band lies inside `[0, state.rows)`, and inside its own block —
///   `block_row + rows() <= block.rows`;
/// * a block wholly above, wholly below, or wholly eroded emits **nothing**, and emits an
///   identical band again if it comes back;
/// * scrolling by one row moves a band by exactly one row until it clips against an edge,
///   at which point it shortens rather than sliding;
/// * `rows == 0` blocks, an empty slice, a zero-row viewport and the **alt screen** all
///   yield an empty result;
/// * blocks may arrive in any order and may overlap; see the module doc.
pub fn visible_blocks(blocks: &[Block], state: ViewState) -> Vec<BlockBand> {
    if state.rows == 0 || state.alt_screen {
        return Vec::new();
    }

    let top = state.first_line();
    let bottom = state.last_line();
    let oldest = state.oldest_retained_line();

    let mut out = Vec::with_capacity(blocks.len());
    for (i, b) in blocks.iter().enumerate() {
        if b.rows == 0 {
            continue;
        }
        // Erosion first, then the viewport clip. The first is implied by the second for
        // any self-consistent `ViewState` (module doc); it is here so that an
        // inconsistent one degrades into "shows less" rather than into a wrong slice.
        let live_first = b.first_abs.max(oldest);
        let live_end = b.end_abs();
        if live_end <= live_first {
            continue; // eroded away entirely
        }
        let vis_first = live_first.max(top);
        let vis_last = (live_end - 1).min(bottom);
        if vis_first > vis_last {
            continue; // wholly above or wholly below the viewport
        }
        // Both differences are bounded: `vis_last <= bottom == top + rows - 1`, and
        // `vis_first` lies inside the block, whose length is a `u16`. A `first_abs` large
        // enough to saturate `end_abs` is also far past `bottom`, so it exits above.
        out.push(BlockBand {
            block: i,
            first_vrow: (vis_first - top) as u16,
            last_vrow: (vis_last - top) as u16,
            block_row: (vis_first - b.first_abs) as u16,
        });
    }
    out
}

/// The first pair of blocks (by input order) whose reserved lines intersect, if any.
///
/// O(n²) and meant for the **insertion** site or a `debug_assert!`, not for the paint
/// loop — the block list is a handful of entries and an overlap is a bookkeeping bug, not
/// a state to render. `rows == 0` blocks occupy nothing and so never collide.
pub fn overlapping(blocks: &[Block]) -> Option<(usize, usize)> {
    for (i, a) in blocks.iter().enumerate() {
        if a.rows == 0 {
            continue;
        }
        for (j, b) in blocks.iter().enumerate().skip(i + 1) {
            if b.rows == 0 {
                continue;
            }
            if a.first_abs < b.end_abs() && b.first_abs < a.end_abs() {
                return Some((i, j));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A live-edge view: `history` lines have scrolled off the screen into scrollback,
    /// `dropped` of them off the top of scrollback entirely.
    fn live(rows: u16, history: usize, dropped: u64) -> ViewState {
        ViewState { rows, display_offset: 0, history, dropped, alt_screen: false }
    }

    fn block(first_abs: u64, rows: u16) -> Block {
        Block { first_abs, rows }
    }

    /// The definition, spelled out one viewport row at a time: which block owns this row,
    /// and which of the block's own rows is it? Later blocks win, because bands are
    /// painted in input order. This is the reference [`visible_blocks`] is checked against
    /// everywhere below.
    fn per_row(blocks: &[Block], st: ViewState) -> Vec<Option<(usize, u16)>> {
        if st.alt_screen {
            return vec![None; usize::from(st.rows)];
        }
        (0..u64::from(st.rows))
            .map(|v| {
                let abs = st.first_line() + v;
                let mut hit = None;
                for (i, b) in blocks.iter().enumerate() {
                    if b.rows == 0 {
                        continue;
                    }
                    let start = b.first_abs.max(st.oldest_retained_line());
                    if abs >= start && abs < b.end_abs() {
                        hit = Some((i, (abs - b.first_abs) as u16));
                    }
                }
                hit
            })
            .collect()
    }

    /// Paint the bands into a row buffer, in the order given — the painter's own loop.
    fn expand(out: &[BlockBand], rows: u16) -> Vec<Option<(usize, u16)>> {
        let mut buf = vec![None; usize::from(rows)];
        for band in out {
            for v in band.first_vrow..=band.last_vrow {
                buf[usize::from(v)] = Some((band.block, band.block_row + (v - band.first_vrow)));
            }
        }
        buf
    }

    /// The invariants that must hold for every band of every frame, whatever the input.
    fn assert_sound(out: &[BlockBand], blocks: &[Block], st: ViewState) {
        for band in out {
            assert!(band.first_vrow <= band.last_vrow, "empty or inverted band: {band:?}");
            assert!(band.last_vrow < st.rows, "band escapes the viewport: {band:?}");
            let b = blocks[band.block];
            assert!(
                u32::from(band.block_row) + u32::from(band.rows()) <= u32::from(b.rows),
                "band {band:?} samples past the end of {b:?}"
            );
        }
        assert_eq!(
            expand(out, st.rows),
            per_row(blocks, st),
            "bands disagree with the per-row definition: {out:?}"
        );
    }

    #[test]
    fn a_block_wholly_inside_the_viewport_reports_its_whole_extent() {
        // 100 lines have scrolled off; the viewport shows absolute 100..110.
        let st = live(10, 100, 0);
        assert_eq!((st.first_line(), st.last_line()), (100, 109));

        let bs = [block(103, 4)];
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![BlockBand { block: 0, first_vrow: 3, last_vrow: 6, block_row: 0 }]
        );
        assert_eq!(out[0].rows(), 4);
        assert_sound(&out, &bs, st);
    }

    #[test]
    fn a_block_entirely_above_or_below_the_viewport_is_not_visible() {
        let st = live(10, 100, 0); // shows 100..=109
        for above in [block(90, 10), block(0, 100), block(99, 1)] {
            assert!(visible_blocks(&[above], st).is_empty(), "{above:?} ends before row 0");
        }
        for below in [block(110, 4), block(110, 1), block(9_000, 40)] {
            assert!(visible_blocks(&[below], st).is_empty(), "{below:?} starts after the last row");
        }
        // The two lines either side of the edge, to pin the off-by-one in both directions.
        assert_eq!(visible_blocks(&[block(91, 10)], st)[0].block_row, 9);
        assert_eq!(visible_blocks(&[block(109, 6)], st)[0].first_vrow, 9);
    }

    /// The half-scrolled case the `block_row` field exists for: the painter must know
    /// which slice of the texture is showing, not merely where to put it.
    #[test]
    fn a_block_clipped_at_the_top_reports_the_offset_into_itself() {
        let st = live(10, 100, 0); // shows 100..=109
        let bs = [block(96, 8)]; // rows 96..=103; the top four are above the viewport
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![BlockBand { block: 0, first_vrow: 0, last_vrow: 3, block_row: 4 }],
            "four rows showing, and they are the block's rows 4..=7"
        );
        assert_sound(&out, &bs, st);
    }

    #[test]
    fn a_block_clipped_at_the_bottom_keeps_block_row_zero() {
        let st = live(10, 100, 0);
        let bs = [block(107, 9)]; // rows 107..=115; only the first three are on screen
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![BlockBand { block: 0, first_vrow: 7, last_vrow: 9, block_row: 0 }],
            "the block's own top is on screen, so the slice starts at 0"
        );
        assert_sound(&out, &bs, st);
    }

    /// A block taller than the screen: clipped at *both* edges, and the slice is the
    /// middle of it.
    #[test]
    fn a_block_taller_than_the_viewport_shows_its_middle() {
        let st = live(10, 100, 0);
        let bs = [block(95, 40)];
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![BlockBand { block: 0, first_vrow: 0, last_vrow: 9, block_row: 5 }]
        );
        assert_eq!(out[0].rows(), 10);
        assert_sound(&out, &bs, st);
    }

    #[test]
    fn nothing_to_show_yields_nothing() {
        let st = live(10, 100, 0);
        assert!(visible_blocks(&[], st).is_empty(), "an empty slice");
        assert!(visible_blocks(&[block(103, 0)], st).is_empty(), "a block reserving no rows");
        assert!(
            visible_blocks(&[block(103, 4)], live(0, 100, 0)).is_empty(),
            "a viewport with no rows"
        );
        // A zero-row block among real ones is skipped, and does not disturb the indices.
        let bs = [block(103, 0), block(104, 2)];
        assert_eq!(
            visible_blocks(&bs, st),
            vec![BlockBand { block: 1, first_vrow: 4, last_vrow: 5, block_row: 0 }]
        );
        assert_sound(&visible_blocks(&bs, st), &bs, st);
    }

    #[test]
    fn the_alt_screen_yields_nothing_whatever_the_geometry() {
        let bs = [block(0, 4), block(100, 8), block(9_000, 2)];
        // Deliberately nonsense scrollback geometry: the alt grid has none, and the answer
        // must not depend on any of it.
        for (history, offset, dropped) in [(0usize, 0usize, 0u64), (900, 400, 12), (0, 77, 4_000)] {
            let st = ViewState { rows: 30, display_offset: offset, history, dropped, alt_screen: true };
            assert!(visible_blocks(&bs, st).is_empty());
            assert_sound(&visible_blocks(&bs, st), &bs, st);
        }
        // And the blocks come back unchanged when the primary grid does — nothing about a
        // block was ever stored in the grid, so there is nothing to restore.
        // `screen_top` is 912, so this scrolls the viewport back onto absolute 100..=129.
        let primary =
            ViewState { rows: 30, display_offset: 812, history: 900, dropped: 12, alt_screen: false };
        assert_eq!(primary.first_line(), 100);
        let back = visible_blocks(&bs, primary);
        assert_eq!(back, vec![BlockBand { block: 1, first_vrow: 0, last_vrow: 7, block_row: 0 }]);
        assert_sound(&back, &bs, primary);
    }

    /// One row of scroll = one row of movement, exactly, until the block hits an edge —
    /// then it shortens instead of sliding, and its `block_row` starts to climb.
    #[test]
    fn scrolling_moves_a_block_one_row_at_a_time_then_clips() {
        let rows = 10u16;
        let history = 200usize;
        let bs = [block(150, 6)];

        let mut previous: Option<BlockBand> = None;
        for offset in 0..=history {
            let st = ViewState { rows, display_offset: offset, history, dropped: 0, alt_screen: false };
            let out = visible_blocks(&bs, st);
            assert_sound(&out, &bs, st);
            let now = out.first().copied();

            if let (Some(p), Some(n)) = (previous, now) {
                // Scrolling back moves the text DOWN the screen by one row per step.
                if p.block_row == 0 && n.block_row == 0 && p.rows() == n.rows() {
                    assert_eq!(n.first_vrow, p.first_vrow + 1, "offset {offset}: exactly one row");
                } else {
                    // Against an edge: it shortens or reveals a different slice, but the
                    // band never grows beyond the block and never leaves the viewport.
                    assert!(n.rows() <= bs[0].rows);
                }
            }
            previous = now;
        }
    }

    /// Scroll a block clean off the top of the screen and back: it must vanish, and it
    /// must return byte-identical. Nothing about a block is stored anywhere, so "the same
    /// view gives the same answer" is the whole of its persistence.
    #[test]
    fn a_block_scrolled_past_an_edge_disappears_and_returns_identically() {
        let history = 300usize;
        let bs = [block(200, 5)];
        let view = |offset: usize| ViewState {
            rows: 12,
            display_offset: offset,
            history,
            dropped: 0,
            alt_screen: false,
        };

        // Sweep the whole scroll range, recording every frame, and prove that the second
        // pass (coming back the other way) reproduces the first exactly.
        let forward: Vec<Vec<BlockBand>> = (0..=history).map(|o| visible_blocks(&bs, view(o))).collect();
        let backward: Vec<Vec<BlockBand>> =
            (0..=history).rev().map(|o| visible_blocks(&bs, view(o))).collect();
        let mut back_in_order = backward;
        back_in_order.reverse();
        assert_eq!(forward, back_in_order, "the direction of travel must not matter");

        assert!(forward.iter().any(|f| f.is_empty()), "it must leave the screen at some point");
        assert!(forward.iter().any(|f| !f.is_empty()), "and be on it at some other point");
        // Fully off the bottom (live edge, block far in the past) and fully off the top
        // (parked above it) are both genuinely empty, not a zero-row band.
        assert!(forward[0].is_empty(), "at the live edge the block is long gone");
        assert!(forward[history].is_empty(), "parked at the very top it is not yet reached");
    }

    /// Erosion eats the top of a block, one row at a time, and never the bottom.
    #[test]
    fn erosion_shortens_from_the_top_and_never_from_the_bottom() {
        let b = block(1_000, 8);
        assert_eq!((b.eroded_rows(0), b.retained_rows(0)), (0, 8), "nothing dropped yet");
        assert_eq!((b.eroded_rows(1_000), b.retained_rows(1_000)), (0, 8), "the first line survives");
        for k in 0..=8u64 {
            let oldest = 1_000 + k;
            assert_eq!(u64::from(b.eroded_rows(oldest)), k, "one row per evicted line");
            assert_eq!(b.retained_rows(oldest), 8 - k as u16);
        }
        assert_eq!(b.eroded_rows(9_999), 8, "erosion cannot exceed the block");
        assert_eq!(b.retained_rows(9_999), 0, "and 0 retained is the reap signal");
        assert_eq!(block(1_000, 0).retained_rows(0), 0, "a zero-row block is born reapable");

        // The bottom is untouched throughout: what is visible is always the block's TAIL.
        let history = 40usize;
        for k in 0..8u64 {
            let st = ViewState {
                rows: 10,
                display_offset: 0,
                history,
                dropped: 1_000 + k,
                alt_screen: false,
            };
            let out = visible_blocks(&[b], st);
            for band in &out {
                assert_eq!(
                    u32::from(band.block_row) + u32::from(band.rows()),
                    u32::from(b.rows),
                    "the block's last row must still be its last row"
                );
            }
        }
    }

    /// The claim made in the module doc, pinned: for any self-consistent `ViewState` the
    /// viewport clip already excludes every eroded line, so the explicit erosion clamp
    /// changes no answer. If this ever fails, one of the two is wrong — do not delete the
    /// clamp to make it pass.
    #[test]
    fn erosion_is_already_implied_by_the_viewport_clip() {
        /// [`visible_blocks`] with the erosion clamp removed.
        fn without_erosion_clamp(blocks: &[Block], st: ViewState) -> Vec<BlockBand> {
            if st.rows == 0 || st.alt_screen {
                return Vec::new();
            }
            let (top, bottom) = (st.first_line(), st.last_line());
            blocks
                .iter()
                .enumerate()
                .filter(|(_, b)| b.rows > 0)
                .filter_map(|(i, b)| {
                    let vis_first = b.first_abs.max(top);
                    let vis_last = (b.end_abs() - 1).min(bottom);
                    (vis_first <= vis_last).then(|| BlockBand {
                        block: i,
                        first_vrow: (vis_first - top) as u16,
                        last_vrow: (vis_last - top) as u16,
                        block_row: (vis_first - b.first_abs) as u16,
                    })
                })
                .collect()
        }

        let bs = [block(0, 6), block(9_998, 9), block(10_002, 3), block(10_040, 30)];
        for dropped in [0u64, 1, 9_999, 10_000, 10_001] {
            for history in [0usize, 1, 40, 10_000] {
                for offset in [0usize, 1, 20, history] {
                    let st = ViewState {
                        rows: 12,
                        display_offset: offset,
                        history,
                        dropped,
                        alt_screen: false,
                    };
                    assert_eq!(
                        visible_blocks(&bs, st),
                        without_erosion_clamp(&bs, st),
                        "dropped {dropped} history {history} offset {offset}"
                    );
                    assert!(
                        st.first_line() >= st.oldest_retained_line(),
                        "…because this is what makes it true"
                    );
                }
            }
        }
    }

    /// Scroll a block through the viewport a row at a time and collect every slice it
    /// reports. The union must be exactly `0..rows` — no row of the texture unreachable,
    /// none reported twice in one frame, and each frame's slice contiguous.
    #[test]
    fn the_reported_slices_tile_the_block_exactly() {
        let history = 60usize;
        let b = block(30, 9);
        let mut seen = vec![false; usize::from(b.rows)];
        for offset in 0..=history {
            let st = ViewState { rows: 5, display_offset: offset, history, dropped: 0, alt_screen: false };
            for band in visible_blocks(&[b], st) {
                for k in 0..band.rows() {
                    seen[usize::from(band.block_row + k)] = true;
                }
                // Contiguity is structural: a band is a range, so it cannot skip a row.
                assert!(band.block_row + band.rows() <= b.rows);
            }
        }
        assert!(seen.iter().all(|s| *s), "every row of the block must be reachable: {seen:?}");
    }

    #[test]
    fn overlapping_finds_the_first_colliding_pair() {
        assert_eq!(overlapping(&[]), None);
        assert_eq!(overlapping(&[block(0, 4), block(4, 4), block(8, 1)]), None, "abutting is not overlapping");
        assert_eq!(overlapping(&[block(0, 4), block(3, 4)]), Some((0, 1)), "one row of overlap");
        assert_eq!(overlapping(&[block(10, 4), block(0, 40)]), Some((0, 1)), "containment, either order");
        assert_eq!(overlapping(&[block(0, 4), block(0, 4)]), Some((0, 1)), "identical blocks");
        assert_eq!(overlapping(&[block(0, 0), block(0, 4)]), None, "a zero-row block occupies nothing");
        assert_eq!(
            overlapping(&[block(100, 2), block(0, 4), block(2, 4), block(1, 9)]),
            Some((1, 2)),
            "the FIRST pair in input order, not the first block with any collision"
        );
    }

    /// Overlap does not break the arithmetic: every band is still individually right, and
    /// the input slice is the z-order — the later block wins the rows they share.
    #[test]
    fn overlapping_blocks_paint_last_wins_and_stay_sound() {
        let st = live(10, 100, 0); // shows 100..=109
        let bs = [block(102, 4), block(104, 4)];
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![
                BlockBand { block: 0, first_vrow: 2, last_vrow: 5, block_row: 0 },
                BlockBand { block: 1, first_vrow: 4, last_vrow: 7, block_row: 0 },
            ],
            "each block reports its own full extent; the painter resolves the overlap"
        );
        assert_sound(&out, &bs, st);
        assert_eq!(overlapping(&bs), Some((0, 1)), "and the caller can know it is a bug");
    }

    /// Blocks arrive in whatever order the caller keeps them; the answer must not care,
    /// and `BlockBand::block` must survive the reordering.
    #[test]
    fn blocks_in_arbitrary_order_each_report_correctly() {
        let st = live(10, 100, 0);
        let bs = [block(107, 2), block(100, 1), block(9, 3), block(103, 2), block(500, 5)];
        let out = visible_blocks(&bs, st);
        assert_eq!(
            out,
            vec![
                BlockBand { block: 0, first_vrow: 7, last_vrow: 8, block_row: 0 },
                BlockBand { block: 1, first_vrow: 0, last_vrow: 0, block_row: 0 },
                BlockBand { block: 3, first_vrow: 3, last_vrow: 4, block_row: 0 },
            ],
            "input order preserved, the two out-of-range blocks silently absent"
        );
        assert_sound(&out, &bs, st);
    }

    /// The sweep: every plausible geometry against every plausible block placement, with
    /// the per-row definition as the oracle. Exhaustive rather than random — the space is
    /// small enough that sampling it would be the weaker test.
    #[test]
    fn every_geometry_agrees_with_the_per_row_definition() {
        let mut frames = 0usize;
        let mut non_empty = 0usize;
        for rows in [0u16, 1, 2, 5, 12] {
            for (history, dropped) in
                [(0usize, 0u64), (4, 0), (12, 0), (12, 7), (40, 10_000), (10_000, 10_000)]
            {
                for offset in [0usize, 1, 3, history / 2, history, history + 5] {
                    let st = ViewState {
                        rows,
                        display_offset: offset,
                        history,
                        dropped,
                        alt_screen: false,
                    };
                    let base = st.first_line();
                    for start in -3i64..=8 {
                        for len in [0u16, 1, 2, 3, 7, 20] {
                            let first_abs = base.saturating_add_signed(start);
                            // Singly, and in company — including a deliberate overlap and
                            // a block sitting on the erosion edge.
                            let singly = [block(first_abs, len)];
                            let together = [
                                block(first_abs, len),
                                block(dropped, 4),
                                block(first_abs.saturating_add(1), len),
                                block(base.saturating_add(4), 2),
                            ];
                            for bs in [&singly[..], &together[..]] {
                                let out = visible_blocks(bs, st);
                                assert_sound(&out, bs, st);
                                frames += 1;
                                non_empty += usize::from(!out.is_empty());
                            }
                        }
                    }
                }
            }
        }
        assert!(frames > 2_000, "the sweep must actually sweep: {frames}");
        assert!(non_empty * 4 > frames, "…and mostly hit, not mostly miss: {non_empty}/{frames}");
    }
}

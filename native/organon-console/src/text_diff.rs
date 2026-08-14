//! Line alignment for one structured edit — the pure half of the diff a tool card draws.
//!
//! **Why this exists.** `Edit` arrives on the event stream as two *fields* —
//! `old_string` and `new_string` — and the card's first rendering printed one as removals
//! and the other as additions with nothing between them. That is honest about what
//! arrived and useless to read: a one-character change inside a ten-line block came out
//! as ten removals followed by ten additions, and a person doing real work in the console
//! hits that on every turn.
//!
//! So the alignment happens here, in a module with no egui, no colours and no widths, and
//! it is tested with plain strings — [`crate::term::encode_key`]'s shape, for the same
//! reason: the decision is the part that can be wrong, and a decision inside a `draw`
//! call can only be checked by looking at it.
//!
//! # The three bounds, and why there are three
//!
//! An `Edit` can carry a large block, and the card lives in a scrollback where one
//! element must not be able to push the conversation off the screen. Each bound answers a
//! different failure, and each one **says what it kept back** rather than quietly cutting:
//!
//! | Bound | Failure it answers | Row it leaves behind |
//! |---|---|---|
//! | [`MAX_CELLS`] | the alignment itself costing more than the card is worth | [`LineDiff::declined`] |
//! | [`MAX_RUN`] | one hunk filling the card, so that only removals are visible and the additions are past the cut | [`DiffRow::Held`] |
//! | [`MAX_ROWS`] | many small hunks filling the card, which no per-hunk bound can catch | [`DiffRow::Held`] |
//!
//! ⚠️ **[`MAX_RUN`] is not redundant with [`MAX_ROWS`], and dropping it is a silent
//! regression rather than a smaller diff.** A global row cap truncates the tail, and in a
//! block replacement every removal precedes every addition — so a global cap alone shows
//! a wall of red and nothing green, which is *worse* than the unaligned rendering it
//! replaced. Capping each same-kind run first is what guarantees both sides of every
//! change are on screen.
//!
//! 📌 **This runs once per card, not once per frame — and it used to be the other way.**
//! [`line_diff`] and the `serde_json` parse beside it in `conversation_view::edit_diff` were
//! both inside the draw call, so every `Edit` card in an unvirtualised scrollback repeated
//! them at 60 Hz. `conversation_view::ConversationPane::diffs` now keeps the result. That is
//! what [`MAX_CELLS`] was originally sized against — 20 000 cells is ~80 KB of scratch and
//! 20 000 comparisons, small beside parsing the JSON the strings came out of — and the
//! budget is left where it is, because the argument for it never depended on the repetition:
//! it is *not* sized against "how large an edit could be", and past it the diff degrades to
//! a block replacement and says so.
//!
//! ⚠️ **One shape is past every bound here and none of the three can see it**, which matters
//! more now that a card pays it once and visibly rather than continuously. [`MAX_CELLS`] is
//! checked **after** the common prefix and suffix are trimmed, so a change with a 400-line
//! common prefix costs *zero* cells, sails past the budget meant to stop large inputs, and
//! then allocates a [`DiffRow::Context`] with an owned `String` for every one of those lines
//! before [`elide`] throws them away. Measured at **78 µs**, nearly twice the largest input
//! the budgets believe is possible. Not fixed here; recorded, with the measurement, in
//! `doc/console_edit_diff_cost.md` §4.

/// Unchanged lines kept either side of a change before the rest are elided.
///
/// Three is `diff -u`'s default and is chosen for the same reason: it is enough to place
/// a change in a file you know and not enough to bury it.
pub const CONTEXT: usize = 3;

/// Consecutive removals — or consecutive additions — drawn before the run says how many
/// it held back.
///
/// Per *run of one kind*, deliberately. See the module doc: a bound on the whole diff
/// cannot keep both halves of a large hunk visible, because in a block replacement they
/// are not interleaved.
pub const MAX_RUN: usize = 8;

/// Rows the whole diff may occupy, elisions and held-back markers included.
pub const MAX_ROWS: usize = 24;

/// The alignment budget, in dynamic-programming cells (`old_lines × new_lines` of the
/// changed region, *after* the common prefix and suffix are trimmed off).
///
/// 20 000 cells is a 141 × 141-line change — far past any edit seen in practice, because
/// the trim removes everything a hunk has in common with its surroundings before this is
/// consulted.
pub const MAX_CELLS: usize = 20_000;

/// One row of a rendered diff. The view chooses colours and prefixes; this says what the
/// row *is*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    /// A line both sides have, kept to place a change.
    Context(String),
    /// A line only `old_string` has.
    Removed(String),
    /// A line only `new_string` has.
    Added(String),
    /// `n` unchanged lines between two changes, summarised rather than drawn.
    Elided(usize),
    /// `n` rows a bound kept back. Distinct from [`Self::Elided`] because the two are
    /// different facts: one is context nobody needs, the other is content the card
    /// refused to draw.
    Held(usize),
}

/// One aligned edit: the rows to draw, and everything about the diff a card may want to
/// say that is not a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineDiff {
    pub rows: Vec<DiffRow>,
    /// Lines in `old_string` and not in `new_string`, counted on the **uncapped**
    /// alignment — so a card can report the size of a change it only partly drew.
    pub removed: usize,
    /// The same, the other way.
    pub added: usize,
    /// `Some((old_lines, new_lines))` when the changed region was past [`MAX_CELLS`] and
    /// was rendered as a block replacement instead of an alignment. The two sizes are
    /// carried so a card can say *what* it refused rather than only that it refused.
    pub declined: Option<(usize, usize)>,
    /// `old_string == new_string`, byte for byte. [`Self::rows`] is empty — there is
    /// nothing to draw, and drawing the block twice with no marks on it is the noise
    /// this field exists to prevent.
    pub unchanged: bool,
    /// The two differ, but in no non-whitespace character: a re-indent, a stripped
    /// trailing space, a line ending, a trailing newline.
    ///
    /// ⚠️ **This is why it is computed on the whole strings rather than per row.** The
    /// aligned rows for such an edit read as `- foo` above `+ foo` — visibly identical,
    /// with the reader left to assume the card is broken. A per-row test could only say
    /// "these two look the same"; the strings can say why.
    pub whitespace_only: bool,
}

impl LineDiff {
    /// Whether the diff found anything to show. False for an identical pair, and for one
    /// whose only difference is a trailing newline — `str::lines` cannot see that, so
    /// there is no row for it and [`Self::whitespace_only`] is the whole report.
    pub fn has_changes(&self) -> bool {
        self.removed > 0 || self.added > 0
    }
}

/// Align two blocks of text, line by line.
///
/// The order of operations is the contract, and each step is bounded before the next runs:
/// trim the common prefix and suffix, align what is left, cap each same-kind run, elide
/// long unchanged runs, cap the total.
pub fn line_diff(old: &str, new: &str) -> LineDiff {
    if old == new {
        return LineDiff {
            rows: Vec::new(),
            removed: 0,
            added: 0,
            declined: None,
            unchanged: true,
            whitespace_only: false,
        };
    }
    let whitespace_only = same_but_for_whitespace(old, new);
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let prefix = common_prefix(&old_lines, &new_lines);
    let suffix = common_suffix(&old_lines[prefix..], &new_lines[prefix..]);
    let old_mid = &old_lines[prefix..old_lines.len() - suffix];
    let new_mid = &new_lines[prefix..new_lines.len() - suffix];

    let (middle, declined) = match align(old_mid, new_mid) {
        Some(rows) => (rows, None),
        None => (block_replace(old_mid, new_mid), Some((old_mid.len(), new_mid.len()))),
    };

    let mut rows: Vec<DiffRow> = Vec::with_capacity(old_lines.len() + new_lines.len());
    rows.extend(old_lines[..prefix].iter().map(|l| DiffRow::Context(l.to_string())));
    rows.extend(middle);
    rows.extend(
        old_lines[old_lines.len() - suffix..].iter().map(|l| DiffRow::Context(l.to_string())),
    );

    // Counted here, before any bound touches the list: the two numbers describe the
    // change, not the rendering of it.
    let removed = rows.iter().filter(|r| matches!(r, DiffRow::Removed(_))).count();
    let added = rows.iter().filter(|r| matches!(r, DiffRow::Added(_))).count();

    // No marks anywhere means there is nothing to place, so there is no context worth
    // keeping either — only [`whitespace_only`] has anything to report.
    let rows = if removed == 0 && added == 0 {
        Vec::new()
    } else {
        cap_total(elide(cap_runs(rows, MAX_RUN), CONTEXT), MAX_ROWS)
    };

    LineDiff { rows, removed, added, declined, unchanged: false, whitespace_only }
}

/// Whether the two are the same text once every whitespace character is removed.
fn same_but_for_whitespace(a: &str, b: &str) -> bool {
    a.chars().filter(|c| !c.is_whitespace()).eq(b.chars().filter(|c| !c.is_whitespace()))
}

fn common_prefix(a: &[&str], b: &[&str]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// The common suffix of two slices that have already had their common prefix removed —
/// which is what makes it safe to count backwards without also having to stop at the
/// front.
fn common_suffix(a: &[&str], b: &[&str]) -> usize {
    a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count()
}

/// The longest-common-subsequence alignment of a changed region, or `None` when the
/// region is past [`MAX_CELLS`].
fn align(old: &[&str], new: &[&str]) -> Option<Vec<DiffRow>> {
    let (n, m) = (old.len(), new.len());
    if n == 0 || m == 0 {
        // One side empty: a pure insertion or a pure deletion. There is nothing to align
        // and the table would be a row of zeroes.
        return Some(block_replace(old, new));
    }
    if n.checked_mul(m).is_none_or(|cells| cells > MAX_CELLS) {
        return None;
    }
    // LCS lengths, filled from the far corner so the forward walk below can read them.
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if old[i] == new[j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let mut rows = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if old[i] == new[j] {
            rows.push(DiffRow::Context(old[i].to_string()));
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
            // A tie goes to the removal, so a replaced line reads `- old` then `+ new`
            // rather than the other way round.
            rows.push(DiffRow::Removed(old[i].to_string()));
            i += 1;
        } else {
            rows.push(DiffRow::Added(new[j].to_string()));
            j += 1;
        }
    }
    rows.extend(old[i..].iter().map(|l| DiffRow::Removed(l.to_string())));
    rows.extend(new[j..].iter().map(|l| DiffRow::Added(l.to_string())));
    Some(rows)
}

/// Every line of `old` as a removal, then every line of `new` as an addition — the
/// unaligned rendering, kept as the honest fallback for a region past [`MAX_CELLS`].
fn block_replace(old: &[&str], new: &[&str]) -> Vec<DiffRow> {
    old.iter()
        .map(|l| DiffRow::Removed(l.to_string()))
        .chain(new.iter().map(|l| DiffRow::Added(l.to_string())))
        .collect()
}

/// Cap each maximal run of one changed kind, leaving a [`DiffRow::Held`] behind.
fn cap_runs(rows: Vec<DiffRow>, max: usize) -> Vec<DiffRow> {
    let mut out = Vec::with_capacity(rows.len());
    let mut i = 0;
    while i < rows.len() {
        let kind = std::mem::discriminant(&rows[i]);
        let mut j = i;
        while j < rows.len() && std::mem::discriminant(&rows[j]) == kind {
            j += 1;
        }
        let changed = matches!(rows[i], DiffRow::Removed(_) | DiffRow::Added(_));
        let keep = if changed { (j - i).min(max) } else { j - i };
        out.extend(rows[i..i + keep].iter().cloned());
        if j - i > keep {
            out.push(DiffRow::Held(j - i - keep));
        }
        i = j;
    }
    out
}

/// Replace the interior of every long unchanged run with a [`DiffRow::Elided`] count.
///
/// A run at the very start of the list keeps only its **last** `context` lines and one at
/// the very end only its **first** — there is no change on the outer side for those lines
/// to place.
fn elide(rows: Vec<DiffRow>, context: usize) -> Vec<DiffRow> {
    let mut out = Vec::with_capacity(rows.len());
    let mut i = 0;
    while i < rows.len() {
        if !matches!(rows[i], DiffRow::Context(_)) {
            out.push(rows[i].clone());
            i += 1;
            continue;
        }
        let mut j = i;
        while j < rows.len() && matches!(rows[j], DiffRow::Context(_)) {
            j += 1;
        }
        let run = j - i;
        let head = if i > 0 { context } else { 0 };
        let tail = if j < rows.len() { context } else { 0 };
        if run <= head + tail {
            out.extend(rows[i..j].iter().cloned());
        } else {
            out.extend(rows[i..i + head].iter().cloned());
            out.push(DiffRow::Elided(run - head - tail));
            out.extend(rows[j - tail..j].iter().cloned());
        }
        i = j;
    }
    out
}

/// Cap the row list, counting what is dropped in **rows** — so a held-back marker that
/// itself falls past the cut contributes the rows it stood for rather than one.
fn cap_total(mut rows: Vec<DiffRow>, max: usize) -> Vec<DiffRow> {
    if rows.len() <= max {
        return rows;
    }
    let held: usize = rows[max..]
        .iter()
        .map(|r| match r {
            DiffRow::Held(n) => *n,
            _ => 1,
        })
        .sum();
    rows.truncate(max);
    rows.push(DiffRow::Held(held));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` numbered lines, joined — a block big enough that an unaligned rendering of it
    /// is unreadable.
    fn block(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
    }

    fn removed(diff: &LineDiff) -> Vec<&str> {
        diff.rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Removed(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn added(diff: &LineDiff) -> Vec<&str> {
        diff.rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Added(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// **CONTRACT — the whole reason this module exists.** A one-character change inside a
    /// long block is one removal and one addition, not the whole block twice.
    #[test]
    fn one_changed_character_in_a_long_block_is_one_changed_line() {
        let old = block(10);
        let new = old.replace("line 5", "line 5!");
        let diff = line_diff(&old, &new);

        assert_eq!(diff.removed, 1, "one line differs, so one line is removed");
        assert_eq!(diff.added, 1, "and one is added");
        assert_eq!(removed(&diff), vec!["line 5"]);
        assert_eq!(added(&diff), vec!["line 5!"]);
        assert!(
            diff.rows.len() <= 2 * CONTEXT + 4,
            "the change plus its context plus two elisions, and nothing else: {:?}",
            diff.rows
        );
        assert!(!diff.unchanged);
        assert!(!diff.whitespace_only);
        assert_eq!(diff.declined, None);
    }

    /// **CONTRACT.** The same change 200 lines into a 400-line block costs the same rows.
    /// A diff's size is the size of the *change*, not of the block it sits in.
    #[test]
    fn a_change_deep_in_a_huge_block_costs_the_same_as_a_change_in_a_small_one() {
        let old = block(400);
        let new = old.replace("line 200\n", "line 200 changed\n");
        let diff = line_diff(&old, &new);

        assert_eq!((diff.removed, diff.added), (1, 1));
        assert!(diff.rows.len() <= MAX_ROWS, "capped: {}", diff.rows.len());
        // The elisions report the whole block, so nothing is silently absent.
        let elided: usize = diff
            .rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Elided(n) => Some(*n),
                _ => None,
            })
            .sum();
        let context = diff.rows.iter().filter(|r| matches!(r, DiffRow::Context(_))).count();
        assert_eq!(elided + context, 399, "every unchanged line is either shown or counted");
    }

    /// **CONTRACT.** Context is what places a change; a change at the very top of a block
    /// has none above it and the diff does not invent an elision for zero lines.
    #[test]
    fn a_change_on_the_first_line_has_no_context_above_it() {
        let old = block(8);
        let new = old.replacen("line 1", "first", 1);
        let diff = line_diff(&old, &new);

        assert!(
            matches!(diff.rows.first(), Some(DiffRow::Removed(t)) if t == "line 1"),
            "the diff opens on the change itself: {:?}",
            diff.rows
        );
        assert_eq!((diff.removed, diff.added), (1, 1));
    }

    /// **CONTRACT.** An inserted line is an addition alone — the lines around it are not
    /// removed and re-added because they moved.
    #[test]
    fn an_inserted_line_removes_nothing() {
        let old = "alpha\nbeta\ngamma";
        let new = "alpha\nbeta\nBETA AND A HALF\ngamma";
        let diff = line_diff(old, new);

        assert_eq!(diff.removed, 0, "nothing was taken away");
        assert_eq!(added(&diff), vec!["BETA AND A HALF"]);
    }

    /// **CONTRACT.** Two changes far apart are two hunks, and the unchanged run between
    /// them is elided rather than drawn.
    #[test]
    fn two_distant_changes_keep_context_around_each_and_elide_between() {
        let old = block(40);
        let new = old.replace("line 5\n", "line 5 X\n").replace("line 35\n", "line 35 Y\n");
        let diff = line_diff(&old, &new);

        assert_eq!((diff.removed, diff.added), (2, 2));
        let elisions = diff.rows.iter().filter(|r| matches!(r, DiffRow::Elided(_))).count();
        assert_eq!(elisions, 3, "above the first change, between the two, below the last");
    }

    /// **CONTRACT.** An identical pair renders nothing at all. The old rendering printed
    /// the block as removals *and* as additions, which is the loudest possible way to say
    /// that nothing happened.
    #[test]
    fn an_identical_pair_says_so_and_draws_no_rows() {
        let diff = line_diff("same\ntext", "same\ntext");
        assert!(diff.unchanged);
        assert!(!diff.has_changes());
        assert!(diff.rows.is_empty(), "{:?}", diff.rows);
        assert!(!diff.whitespace_only, "identical is not a whitespace difference");
    }

    /// **CONTRACT.** A re-indent is reported as a whitespace difference, because its rows
    /// are visibly identical and a reader with no note would read the card as broken.
    #[test]
    fn a_reindent_is_named_as_whitespace_rather_than_left_to_look_like_a_bug() {
        let diff = line_diff("if x:\n  return 1", "if x:\n    return 1");
        assert!(diff.whitespace_only);
        assert!(!diff.unchanged, "the strings genuinely differ");
        assert_eq!((diff.removed, diff.added), (1, 1), "the changed line is still shown");
    }

    /// **CONTRACT.** `str::lines` cannot see a trailing newline, so a diff that has no row
    /// to show for it must still not claim the two are identical.
    #[test]
    fn a_trailing_newline_difference_is_not_reported_as_no_change() {
        let diff = line_diff("body\n", "body");
        assert!(!diff.unchanged, "the strings differ by a byte");
        assert!(diff.whitespace_only, "and the byte is whitespace");
        assert!(!diff.has_changes());
        assert!(diff.rows.is_empty(), "no line-level row can honestly show this");
    }

    /// **CONTRACT.** A changed word is a real change even when whitespace also moved.
    #[test]
    fn a_real_change_beside_a_whitespace_change_is_not_called_whitespace_only() {
        let diff = line_diff("  alpha", "beta");
        assert!(!diff.whitespace_only);
        assert!(diff.has_changes());
    }

    /// **CONTRACT.** One hunk may not fill the card with removals and push every addition
    /// past the cut — the failure a global row cap alone would produce.
    #[test]
    fn a_large_hunk_shows_both_of_its_halves() {
        let old = (1..=60).map(|i| format!("was {i}")).collect::<Vec<_>>().join("\n");
        let new = (1..=60).map(|i| format!("now {i}")).collect::<Vec<_>>().join("\n");
        let diff = line_diff(&old, &new);

        assert_eq!((diff.removed, diff.added), (60, 60), "the counts are of the change");
        assert_eq!(removed(&diff).len(), MAX_RUN, "the removals are capped per run");
        assert!(!added(&diff).is_empty(), "and the additions are still on screen");
        assert!(diff.rows.len() <= MAX_ROWS + 1);
    }

    /// **CONTRACT.** Every bound leaves a marker. A card that clipped and said nothing is
    /// indistinguishable from one that showed the whole edit.
    #[test]
    fn every_bound_says_what_it_kept_back() {
        let old = (1..=60).map(|i| format!("was {i}")).collect::<Vec<_>>().join("\n");
        let new = (1..=60).map(|i| format!("now {i}")).collect::<Vec<_>>().join("\n");
        let diff = line_diff(&old, &new);
        assert!(
            diff.rows.iter().any(|r| matches!(r, DiffRow::Held(_))),
            "something was held back and nothing said so: {:?}",
            diff.rows
        );
    }

    /// **CONTRACT.** Many small hunks are bounded too — the case no per-hunk cap catches,
    /// since every run here is one row long and `MAX_RUN` never fires.
    ///
    /// 📌 Sized to stay *inside* the alignment budget on purpose (a 60-line block, every
    /// fourth line changed, so the trimmed region is 57 × 57 = 3 249 cells). A larger one
    /// would decline to align and would then be testing the other path.
    #[test]
    fn many_small_hunks_are_bounded_by_the_total() {
        let old = block(60);
        let new = (1..=60)
            .map(|i| if i % 4 == 0 { format!("line {i} changed") } else { format!("line {i}") })
            .collect::<Vec<_>>()
            .join("\n");
        let diff = line_diff(&old, &new);

        assert_eq!(diff.declined, None, "inside the budget, so genuinely aligned");
        assert_eq!((diff.removed, diff.added), (15, 15), "fifteen one-line hunks");
        assert!(diff.rows.len() <= MAX_ROWS + 1, "{} rows", diff.rows.len());
        assert!(
            diff.rows.iter().any(|r| matches!(r, DiffRow::Held(_))),
            "the total cap fired, and said so: {:?}",
            diff.rows
        );
    }

    /// **CONTRACT.** Past the alignment budget the diff degrades to a block replacement
    /// and *names the sizes it refused*, rather than aligning something enormous or
    /// pretending the edit was small.
    #[test]
    fn a_change_region_past_the_budget_declines_to_align_and_says_the_sizes() {
        let side = 200; // 200 × 200 = 40 000 cells, past MAX_CELLS
        let old = (1..=side).map(|i| format!("old {i}")).collect::<Vec<_>>().join("\n");
        let new = (1..=side).map(|i| format!("new {i}")).collect::<Vec<_>>().join("\n");
        let diff = line_diff(&old, &new);

        assert_eq!(diff.declined, Some((side, side)));
        assert_eq!((diff.removed, diff.added), (side, side));
        assert!(!removed(&diff).is_empty() && !added(&diff).is_empty(), "both halves shown");
    }

    /// **CONTRACT.** The budget is consulted **after** the prefix/suffix trim, so a
    /// one-line change in a 2 000-line block still aligns.
    #[test]
    fn the_budget_is_measured_on_the_change_and_not_on_the_block() {
        let old = block(2_000);
        let new = old.replace("line 1000\n", "line 1000 changed\n");
        let diff = line_diff(&old, &new);
        assert_eq!(diff.declined, None, "2000 × 2000 cells were never allocated");
        assert_eq!((diff.removed, diff.added), (1, 1));
    }

    /// **CONTRACT.** An edit into an empty string, and out of one, are a pure insertion
    /// and a pure deletion — not an alignment against nothing.
    #[test]
    fn an_empty_side_is_a_pure_insertion_or_deletion() {
        let insert = line_diff("", "one\ntwo");
        assert_eq!((insert.removed, insert.added), (0, 2));
        let delete = line_diff("one\ntwo", "");
        assert_eq!((delete.removed, delete.added), (2, 0));
    }
}

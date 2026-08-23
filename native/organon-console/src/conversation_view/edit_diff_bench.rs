//! **What an `Edit` card costs, and what it used to cost every single frame.**
//!
//! A measurement, not a feature — and the measurement that decided
//! [`ConversationPane::diffs`] should exist.
//!
//! [`super::tool_card`] used to call [`super::edit_diff`] from its own body, so an `Edit`
//! card re-parsed its whole arguments blob with `serde_json` and re-ran
//! [`crate::text_diff::line_diff`] over the result on every draw, keeping the answer
//! nowhere. The scrollback is not virtualised — `egui::ScrollArea::show` lays out every
//! element — so a session holding *n* `Edit` cards paid all *n* of them, sixty times a
//! second, however few were visible.
//!
//! `text_diff` already bounds a *single* diff ([`crate::text_diff::MAX_CELLS`],
//! [`MAX_ROWS`](crate::text_diff::MAX_ROWS), [`MAX_RUN`](crate::text_diff::MAX_RUN)), and
//! its module doc argued the recomputation was deliberate and that the budget was sized
//! against it. That was a defensible claim about **one** card. The open question was never
//! the worst single case; it was **the multiplier** — how many cards, at what each, against
//! a 16.7 ms frame — and that is what this module answered.
//!
//! 🚨 **It still answers it, on both sides of the change.** [`Cache::Off`] clears the pane's
//! map at the top of every frame, which *is* the old code rather than a model of it, so the
//! before-and-after can be re-taken instead of believed. That mode is the reason this module
//! did not become a historical curiosity the moment the cache landed.
//!
//! The written finding is `doc/console_edit_diff_cost.md`. This module is the instrument
//! that produced its numbers, kept in the tree so they can be re-taken on another machine
//! or after a dependency bump.
//!
//! # What is real here, and what is not
//!
//! **Real:** the function under test. [`super::edit_diff`] is called with the exact
//! [`Arguments`] a `ToolCall` fold produces, and the whole-frame condition drives
//! [`super::scrollback`] — the same function `conversation_view::draw` calls — over a real
//! [`Transcript`] inside a real [`egui::ScrollArea`]. No part of the path is approximated.
//!
//! **Not real:** the *words*. The corpus is written here rather than captured; the fixtures
//! in `fixtures/` are 11–77 lines and no long session has been captured on this machine.
//! What is taken from life is the **shapes** — §"the five shapes" below says where each one
//! comes from and how common it is.
//!
//! **Not measured:** anything on a GPU. [`egui::Context::run`] stops at shapes; nothing is
//! tessellated and no swapchain exists. The question is CPU, and this is the envelope that
//! contains it.
//!
//! ⚠️ **Difference within a corpus, never across two.** The `uncached − cached` figure is
//! exact, because both columns run the identical transcript and differ only in whether the
//! map survives a frame. The `vs Read` column is **not** exact and must not be read as
//! `edit_diff`'s cost: an `Edit` card and a `Read` card draw different numbers of rows, and
//! a corpus of large edits holds three orders of magnitude more argument text in memory than
//! a `Read` one, which is slower at everything. `Read` is there as a control on how far a
//! given run drifted, and as the budget the saving is read against — not as a baseline to
//! subtract. `doc/console_edit_diff_cost.md` §5.2 works the trap through.
//!
//! # Running it
//!
//! ```text
//! cargo test --release -p organon-console --lib -- --ignored --nocapture edit_diff_bench
//! ```
//!
//! ⚠️ **`--release`, because the console ships release.** The workspace's `[profile.dev]`
//! is `opt-level = 1` and the test profile inherits it; the document records both and quotes
//! only the release figures.
//!
//! `#[ignore]` on the tables because they are seconds and the default suite is a correctness
//! gate. The tests here that are **not** ignored are fast and pin the properties the
//! document's reasoning rests on — that the corpus is the shapes it claims to be, that the
//! whole transcript really is laid out, and that the cache is at the call site rather than
//! inside [`super::edit_diff`]. The cache's own **correctness** tests are not here: they are
//! in [`super::tests`] with the rest of the view's contracts, and they drive [`frame`] and
//! [`super::rewrap_bench::bench_pane`] rather than building a pane of their own.
//!
//! # Its relationship to [`super::rewrap_bench`]
//!
//! Siblings, not layers. `doc/console_rewrap_measurement.md` §6 and §8 found this cost,
//! named it, and deliberately excluded it — that benchmark asks what a change of **width**
//! costs and takes a corpus of `Read` cards. This one asks what a tool card's own
//! **derivation** costs and takes five shapes of `Edit`.
//!
//! **What is shared is [`super::rewrap_bench::bench_pane`]**, because it enumerates every field of
//! [`ConversationPane`] and is therefore the one piece that rots when the struct changes —
//! as it did when `diffs` landed. **What is not shared is the frame driver**, deliberately:
//! that module's takes a width and imposes an inset, this one's takes a [`Cache`] and
//! imposes nothing, and a merged driver would hand each measurement a parameter it does not
//! use and must remember to hold still.

use std::hint::black_box;
use std::time::{Duration, Instant};

use super::rewrap_bench::bench_pane;
use super::{edit_diff, scrollback, ConversationPane, SurfaceImages};
use crate::conversation::{
    AgentEvent, Arguments, MessageId, ResultDetail, RunOutcome, ToolId, Transcript,
};
use crate::posture::Form;
use crate::text_diff;
use crate::theme::Theme;

/// The pane the console opens at, in points — `organon-console`'s default window is
/// 1100×720 and the conversation front-end fills it.
const PANE_W: f32 = 1100.0;
const PANE_H: f32 = 720.0;

/// Frames run and thrown away before any timing is kept.
///
/// ⚠️ Not a ritual number. The first frame of a fresh [`egui::Context`] rasterises glyphs
/// into an empty font atlas and allocates every widget's memory for the first time; frames
/// two and three still settle the `ScrollArea`'s own state.
const WARMUP: usize = 6;

/// A 60 Hz frame, in microseconds — the budget every figure here is read against.
const FRAME_BUDGET_US: f64 = 16_666.7;

// ---------------------------------------------------------------------------
// The five shapes

/// One `Edit` the corpus can be built from: a name, and the two strings the card diffs.
///
/// The five shapes are chosen to span what the *bounds* do, not to average a session:
/// two under every budget, one at the alignment budget's edge, one past it, and one that no
/// budget touches at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// **One line changed inside a small block.** The ordinary edit, and by a long way the
    /// most common — six lines of context around a single replaced line.
    OneLine,
    /// **A function-sized hunk**: thirty lines with three separate changed regions, which is
    /// what a refactor of one function looks like on the wire.
    Hunk,
    /// **At the alignment budget's edge**: 140 × 140 changed lines is 19 600 cells against
    /// [`text_diff::MAX_CELLS`]'s 20 000, so this is the largest diff the bound *permits*
    /// rather than declines. The most expensive single card that can be drawn as an
    /// alignment.
    AtBudget,
    /// **Past the budget**: 200 × 200 is 40 000 cells, so [`text_diff::line_diff`] declines
    /// the alignment and falls back to a block replacement. Included because *declining* is
    /// not free — the fallback still allocates a `String` per line of both sides.
    Declined,
    /// **A long common prefix with one line changed at the end.**
    ///
    /// 🚨 **This is the shape the three bounds do not see.** `MAX_CELLS` is checked
    /// *after* the common prefix and suffix are trimmed, so a 400-line prefix costs zero
    /// cells and passes every budget — and then `line_diff` builds a `DiffRow::Context`
    /// carrying an owned `String` for every one of those 400 lines before `elide` throws
    /// them away. `MAX_ROWS` bounds what is *drawn*, not what is *allocated*.
    ContextHeavy,
}

impl Shape {
    /// All five, in the order the tables print them.
    pub const ALL: [Shape; 5] =
        [Shape::OneLine, Shape::Hunk, Shape::AtBudget, Shape::Declined, Shape::ContextHeavy];

    pub fn label(self) -> &'static str {
        match self {
            Shape::OneLine => "one-line",
            Shape::Hunk => "hunk",
            Shape::AtBudget => "at-budget",
            Shape::Declined => "declined",
            Shape::ContextHeavy => "ctx-heavy",
        }
    }

    /// The `(old_string, new_string)` pair for this shape, unique to `turn`.
    ///
    /// ⚠️ Every string carries its turn index, deliberately. Identical text would let the
    /// allocator's reuse and egui's galley cache both flatter the result — a corpus of
    /// repeated edits would measure deduplication rather than work.
    fn strings(self, turn: usize) -> (String, String) {
        match self {
            Shape::OneLine => {
                let block = |mid: &str| {
                    format!(
                        "fn surface_{turn}(table: &mut Table, key: Key) -> &mut Surface {{\n\
                             let entry = table.entry(key);\n\
                         {mid}\n\
                             entry.or_insert_with(Surface::new)\n\
                         }}\n"
                    )
                };
                (block("    // the width is inherited"), block("    // the width is pinned"))
            }
            Shape::Hunk => {
                let block = |a: &str, b: &str, c: &str| {
                    let mut s = String::new();
                    for line in 0..30 {
                        let body = match line {
                            4 => a.to_string(),
                            14 => b.to_string(),
                            24 => c.to_string(),
                            _ => format!("    let step_{turn}_{line} = pass.record(key);"),
                        };
                        s.push_str(&body);
                        s.push('\n');
                    }
                    s
                };
                (
                    block("    let width = ui.available_width();", "    debug_assert!(w > 0.0);", "    return Ok(width);"),
                    block("    let width = self.pinned_width;", "    debug_assert!(w >= 1.0);", "    return Ok(self.width);"),
                )
            }
            Shape::AtBudget => (lines(turn, 140, "old"), lines(turn, 140, "new")),
            Shape::Declined => (lines(turn, 200, "old"), lines(turn, 200, "new")),
            Shape::ContextHeavy => {
                let prefix = lines(turn, 400, "same");
                (format!("{prefix}    let tail = old_value;\n"), format!("{prefix}    let tail = new_value;\n"))
            }
        }
    }

    /// The complete `Edit` arguments blob, as the harness re-serialises `tool_use.input`.
    fn arguments(self, turn: usize) -> Arguments {
        let (old, new) = self.strings(turn);
        let text = serde_json::json!({
            "file_path": format!("C:\\work\\organon\\native\\src\\part_{turn}.rs"),
            "old_string": old,
            "new_string": new,
        })
        .to_string();
        Arguments { text, complete: true }
    }
}

/// `count` distinct source-looking lines, tagged with `turn` and `side`.
fn lines(turn: usize, count: usize, side: &str) -> String {
    let mut s = String::with_capacity(count * 64);
    for line in 0..count {
        s.push_str(&format!(
            "    let {side}_{turn}_{line} = table.entry(key_{line}).or_insert_with(Surface::new);\n"
        ));
    }
    s
}

// ---------------------------------------------------------------------------
// The direct condition: `edit_diff` alone, no egui

/// What one shape costs per call, split into the two halves a cache would eliminate
/// together.
pub struct DirectCost {
    pub shape: Shape,
    /// `serde_json::from_str` over the whole arguments blob.
    pub parse: Duration,
    /// The whole of [`super::edit_diff`] — parse, field lookups, and `line_diff`.
    pub total: Duration,
    /// Rows the resulting diff actually draws, after every bound. The layout cost the
    /// frame condition adds on top is proportional to this, not to the edit's size.
    pub rows: usize,
    /// Bytes of arguments JSON — what the parse walks.
    pub bytes: usize,
}

impl DirectCost {
    /// `line_diff` and the field lookups, i.e. everything the parse is not.
    pub fn diff(&self) -> Duration {
        self.total.saturating_sub(self.parse)
    }

    fn row(&self) -> String {
        let us = |d: Duration| d.as_secs_f64() * 1e6;
        format!(
            "| {:<10} | {:>7} | {:>5} | {:>9.2} | {:>9.2} | {:>9.2} | {:>8.0} |",
            self.shape.label(),
            self.bytes,
            self.rows,
            us(self.total),
            us(self.parse),
            us(self.diff()),
            FRAME_BUDGET_US / us(self.total),
        )
    }
}

/// Time `edit_diff` for one shape, as the median of `samples` calls.
///
/// ⚠️ **A fresh `Arguments` per sample, and [`black_box`] around the result.** `edit_diff`
/// is pure and its return value is dropped, which is exactly the shape LLVM deletes; and
/// timing one blob repeatedly would measure it warm in L1, which is not how a transcript of
/// distinct cards behaves.
pub fn direct(shape: Shape, samples: usize) -> DirectCost {
    let args: Vec<Arguments> = (0..samples).map(|t| shape.arguments(t)).collect();

    let mut parses = Vec::with_capacity(samples);
    for a in &args {
        let start = Instant::now();
        let v: Option<serde_json::Value> = serde_json::from_str(&a.text).ok();
        black_box(&v);
        parses.push(start.elapsed());
    }

    let mut totals = Vec::with_capacity(samples);
    for a in &args {
        let start = Instant::now();
        let d = edit_diff(Some("Edit"), a);
        black_box(&d);
        totals.push(start.elapsed());
    }

    let rows = edit_diff(Some("Edit"), &args[0]).map_or(0, |d| d.diff.rows.len());
    parses.sort_unstable();
    totals.sort_unstable();
    DirectCost {
        shape,
        parse: parses[parses.len() / 2],
        total: totals[totals.len() / 2],
        rows,
        bytes: args[0].text.len(),
    }
}

// ---------------------------------------------------------------------------
// The corpus, and the pane

/// Elements one synthetic turn contributes: human, prose, tool card + result, prose.
const PER_TURN: usize = 5;

/// What kind of tool card fills a corpus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// `Read` cards — the corpus `doc/console_rewrap_measurement.md` measured, and the
    /// baseline every Edit figure is differenced against.
    Read,
    /// Every card the same [`Shape`]. Isolates that shape; is not a session.
    Every(Shape),
    /// **One card in `n` is [`Shape::AtBudget`]; the rest are [`Shape::OneLine`].**
    ///
    /// 🚨 **A stated distribution, not a measured one.** No long session has been captured
    /// on this machine, so any "realistic mix" here would be invented precision wearing a
    /// measurement's clothes. This says something weaker and checkable instead — *if* one
    /// edit in `n` is large, this is the cost — and leaves the reader to supply `n` from
    /// their own sessions. `n = 10` is what the tables print.
    LargeOneIn(usize),
}

impl Fill {
    fn label(self) -> String {
        match self {
            Fill::Read => "Read".to_string(),
            Fill::Every(s) => format!("Edit {}", s.label()),
            Fill::LargeOneIn(n) => format!("Edit 1-in-{n} big"),
        }
    }

    /// The card for turn `turn`: a tool name and its complete arguments text.
    fn card(self, turn: usize) -> (&'static str, String) {
        let shape = match self {
            Fill::Read => {
                return (
                    "Read",
                    format!(
                        "{{\"file_path\": \"C:\\\\work\\\\organon\\\\native\\\\src\\\\part_{turn}.rs\", \
                         \"offset\": {turn}, \"limit\": 120}}"
                    ),
                )
            }
            Fill::Every(s) => s,
            Fill::LargeOneIn(n) => {
                if n > 0 && turn % n == 0 {
                    Shape::AtBudget
                } else {
                    Shape::OneLine
                }
            }
        };
        ("Edit", shape.arguments(turn).text)
    }
}

/// A transcript of roughly `elements` elements whose tool cards are decided by `fill`.
///
/// Every corpus differs **only** in the tool card, so differencing two of them isolates the
/// card.
fn corpus(elements: usize, fill: Fill) -> Transcript {
    let mut t = Transcript::new();
    t.apply(AgentEvent::SessionStarted { session_id: "edit-diff-bench".into() });
    for turn in 0..elements.div_ceil(PER_TURN) {
        let tool = ToolId(format!("toolu_{turn}"));
        let (name, arguments) = fill.card(turn);
        t.apply(AgentEvent::HumanInput { text: human_text(turn) });
        t.apply(AgentEvent::AssistantMessage {
            message: MessageId(format!("msg_{turn}_0")),
            text: prose_text(turn),
        });
        t.apply(AgentEvent::ToolCall {
            id: tool.clone(),
            name: name.into(),
            arguments: Some(arguments),
        });
        t.apply(AgentEvent::ToolResult {
            id: tool,
            output: output_text(turn),
            is_error: false,
            detail: ResultDetail {
                file_path: Some(format!("C:\\work\\organon\\native\\src\\part_{turn}.rs")),
                lines: Some(120),
                total_lines: Some(2_400),
                start_line: Some(1),
            },
        });
        t.apply(AgentEvent::AssistantMessage {
            message: MessageId(format!("msg_{turn}_1")),
            text: closing_text(turn),
        });
        t.apply(AgentEvent::RunFinished { outcome: RunOutcome::Ok, detail: None });
    }
    t
}

fn human_text(turn: usize) -> String {
    format!(
        "In turn {turn}: change the part of the render path that owns the surface table so \
         the wrap width is pinned there rather than inherited."
    )
}

fn prose_text(turn: usize) -> String {
    format!(
        "Looking at turn {turn}, the surface table is built where the pass is recorded rather \
         than where it is consumed, so the width the caller hands down is already the wrapped \
         width by the time anything reads it. That means the decision is one level up, in the \
         caller, and the table itself only ever sees a number it cannot influence. The \
         consequence worth stating is that a change of available width does not travel through \
         the table at all — it travels through the caller, which rebuilds the whole thing."
    )
}

fn output_text(turn: usize) -> String {
    (0..12)
        .map(|line| {
            format!("{:>5}\tapplied edit {turn} at surface_{turn}_{line}", line + 1)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn closing_text(turn: usize) -> String {
    format!(
        "So for turn {turn} the width is now pinned beside the pass record. I have left the \
         table alone; nothing that reads it has to change."
    )
}


/// Whether the pane keeps its diff cache between frames.
///
/// 🚨 **[`Cache::Off`] is not a simulation of the old code — it *is* the old code.** Before
/// [`ConversationPane::diffs`] existed, `tool_card` called [`edit_diff`] from its own body,
/// so every card recomputed its diff on every frame. Clearing the map at the top of each
/// frame reproduces exactly that and nothing else: the same call, at the same point in the
/// walk, with the same arguments.
///
/// It is here because the numbers that *justified* the cache would otherwise have become
/// unreproducible the moment the cache landed — a document quoting a before-and-after where
/// only the "after" can be re-taken is asking to be believed rather than checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cache {
    /// As the console ships: computed once per card, kept until the fold evicts it.
    On,
    /// As the console shipped before this cache: recomputed per card per frame.
    Off,
}

/// One frame of the scrollback: how long it took, and how many **passes** egui ran inside
/// it.
///
/// ⚠️ The pass count is not decoration. [`egui::Context::run`] re-runs the whole ui when
/// something calls `request_discard`, and a frame that runs twice draws every card twice —
/// which under [`Cache::Off`] would double the `edit_diff` calls and report that as a
/// doubled cost. Every table here prints it so the reader can see it stayed 1.
///
/// `pub(super)` because [`super::tests`]'s diff-cache contracts drive it — see the module
/// doc on what is shared with [`super::rewrap_bench`] and what is not.
pub(super) fn frame(
    ctx: &egui::Context,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
    theme: &Theme,
    cache: Cache,
) -> (Duration, u8) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(PANE_W, PANE_H),
        )),
        ..Default::default()
    };
    let start = Instant::now();
    // ⚠️ **Inside the timing, deliberately.** The old code built an `EditDiff` and dropped
    // it within one frame; with the cache off, the build happens in this frame and the drop
    // in the `clear` at the top of the next one. Timing the clear is what puts both halves
    // back in the same frame — freeing four hundred cards' worth of `String` is real work
    // and it was the old code's, so excluding it would flatter the "before" column.
    if cache == Cache::Off {
        pane.diffs.clear();
    }
    let out = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            // 🚨 **The TERMINAL form, for the reason `rewrap_bench` gives and one of its
            // own.** ✏️ **The margin is no longer among those reasons** — since it moved to
            // the pane a desktop `Form` cannot inset anything from in here, because this
            // harness builds no pane. What a desktop `Form` still changes is every card's
            // padding, corner and line height, which would move the figures without moving
            // the thing the bench varies. Holding it equal to the other bench's also keeps
            // the two documents' figures on one footing.
            let _ = scrollback(ui, pane, images, &Default::default(), theme, &Form::TERMINAL);
        });
    });
    (start.elapsed(), out.platform_output.num_completed_passes as u8)
}

/// What one whole-frame condition measured.
pub struct FrameCost {
    pub elements: usize,
    pub label: String,
    pub median: Duration,
    pub passes: u8,
    /// Tool cards in the corpus — the multiplier the direct figure is paid against.
    pub cards: usize,
}

/// `WARMUP + samples` frames of a corpus, keeping the median of the last `samples`.
pub fn frames(elements: usize, fill: Fill, samples: usize, cache: Cache) -> FrameCost {
    let ctx = egui::Context::default();
    let images = SurfaceImages::new();
    let theme = Theme::organon();
    let transcript = corpus(elements, fill);
    let cards = transcript.elements().iter().filter(|e| e.tool().is_some()).count();
    let mut pane = bench_pane(transcript);
    let mut kept = Vec::with_capacity(samples);
    let mut passes = 0;
    for i in 0..WARMUP + samples {
        let (took, ran) = frame(&ctx, &mut pane, &images, &theme, cache);
        if i >= WARMUP {
            kept.push(took);
            passes = passes.max(ran);
        }
    }
    kept.sort_unstable();
    FrameCost { elements, label: fill.label(), median: kept[kept.len() / 2], passes, cards }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The most rows a diff can actually carry, which is **[`text_diff::MAX_ROWS`] + 1**.
    ///
    /// ⚠️ Not a fudge factor — a measured off-by-one in a constant this module reads, noted
    /// here rather than changed because it belongs to `text_diff` and is invisible on
    /// screen. `cap_total` truncates to `max` and *then* pushes the `Held` marker that says
    /// how much it dropped (`text_diff.rs:312`), so a capped diff is 25 rows while
    /// `MAX_ROWS`'s own doc says "elisions and held-back markers **included**". One row of
    /// card, and nothing here turns on it; it matters only because a test that asserted the
    /// documented bound would fail on correct code.
    fn drawn_row_ceiling() -> usize {
        text_diff::MAX_ROWS + 1
    }

    /// The corpus is the shapes it claims to be: every one parses, every one produces a
    /// diff, and each one lands on the side of the bounds its doc comment says it does.
    ///
    /// 🚨 **Without this the tables are unreadable**, because `edit_diff` returns `None` for
    /// anything it cannot parse and a `None` costs almost nothing — a corpus with a typo in
    /// its JSON would report a wonderfully cheap `edit_diff` and mean nothing at all.
    #[test]
    fn every_shape_is_the_shape_it_says_it_is() {
        for shape in Shape::ALL {
            let args = shape.arguments(7);
            let d = edit_diff(Some("Edit"), &args)
                .unwrap_or_else(|| panic!("{} must parse and diff", shape.label()));
            assert!(
                d.diff.has_changes(),
                "{} must contain a change, or it is measuring the identical-pair early \
                 return instead of a diff",
                shape.label()
            );
            let declined = d.diff.declined.is_some();
            assert_eq!(
                declined,
                shape == Shape::Declined,
                "{} lands on the wrong side of MAX_CELLS: declined = {declined}",
                shape.label()
            );
            assert!(
                d.diff.rows.len() <= drawn_row_ceiling(),
                "{} draws {} rows, past the ceiling — the bound is not holding",
                shape.label(),
                d.diff.rows.len()
            );
        }
    }

    /// 🚨 **[`edit_diff`] itself is still uncached, which is what the direct table measures
    /// and what [`Cache::Off`] reproduces.**
    ///
    /// The cache added by [`ConversationPane::diffs`] lives at the *call site*, not in this
    /// function — deliberately, so that the pure function stays pure and testable, and so
    /// that the walk in [`scrollback`] is the single place that decides how long an answer
    /// lives. This test pins that split: two calls do the work twice and return two
    /// separately-owned equal values.
    ///
    /// Its job is to fail if someone later memoises `edit_diff` internally, at which point
    /// there would be **two** caches with two invalidation stories, the `Cache::Off` column
    /// would silently stop reproducing the old code, and the before-and-after in
    /// `doc/console_edit_diff_cost.md` would be measuring nothing.
    #[test]
    fn the_pure_function_is_still_uncached_and_the_cache_is_at_the_call_site() {
        let args = Shape::Hunk.arguments(3);
        let first = edit_diff(Some("Edit"), &args).expect("a diff");
        let second = edit_diff(Some("Edit"), &args).expect("a diff");
        assert_eq!(first.diff, second.diff, "the same input must give the same diff");
        assert!(
            !std::ptr::eq(first.diff.rows.as_ptr(), second.diff.rows.as_ptr()),
            "two calls returned the same allocation — `edit_diff` has grown a cache of its \
             own, and the one in ConversationPane::diffs is now the second of two"
        );
    }

    /// 🚨 **The context-heavy shape is past every bound, and the bounds do not see it.**
    ///
    /// 400 lines of common prefix trim to **zero** alignment cells, so `MAX_CELLS` never
    /// fires; `MAX_ROWS` then caps what is *drawn* to a handful. The cost is in between, in
    /// the `Vec<DiffRow>` `line_diff` builds before `elide` discards it — invisible to every
    /// bound and to the drawn output alike. This test states the gap so that a future
    /// attempt to bound the prefix has something that fails when it succeeds.
    #[test]
    fn a_long_common_prefix_is_bounded_by_nothing_until_after_it_is_built() {
        let args = Shape::ContextHeavy.arguments(0);
        let d = edit_diff(Some("Edit"), &args).expect("a diff");
        assert!(d.diff.declined.is_none(), "a long prefix costs zero cells, so it is never declined");
        assert!(
            d.diff.rows.len() <= drawn_row_ceiling(),
            "MAX_ROWS still bounds the drawing: {} rows",
            d.diff.rows.len()
        );
        let (old, _) = Shape::ContextHeavy.strings(0);
        assert!(
            old.lines().count() > 20 * d.diff.rows.len(),
            "the shape must be far larger than what it draws, or it is not probing the gap"
        );
    }

    /// A whole-frame condition really does lay out every card, on screen or not.
    ///
    /// The 720 pt viewport shows perhaps a dozen elements; if egui culled, the frame cost
    /// would not move with the transcript and the multiplier this module is about would not
    /// exist.
    #[test]
    fn the_whole_transcript_is_drawn_not_just_the_visible_slice() {
        let small = frames(20, Fill::Every(Shape::AtBudget), 3, Cache::Off);
        let large = frames(200, Fill::Every(Shape::AtBudget), 3, Cache::Off);
        assert!(large.cards > small.cards * 4, "the corpus must scale: {} vs {}", small.cards, large.cards);
        assert!(
            large.median > small.median * 3,
            "ten times the transcript must cost far more — otherwise something is culling \
             and this whole measurement is about the wrong thing: {:?} vs {:?}",
            small.median,
            large.median
        );
    }

    /// **The direct measurement**: what one `edit_diff` costs, by shape.
    ///
    /// `cargo test --release -p organon-console --lib -- --ignored --nocapture edit_diff_bench`
    #[test]
    #[ignore = "a benchmark, not a correctness check — see doc/console_edit_diff_cost.md"]
    fn edit_diff_direct_cost_table() {
        println!(
            "\n| shape      |   bytes | rows |  total µs |  parse µs |   diff µs | per frame |"
        );
        println!(
            "|------------|---------|------|-----------|-----------|-----------|-----------|"
        );
        for shape in Shape::ALL {
            println!("{}", direct(shape, 200).row());
        }
        println!();
    }

    /// **The frame measurement**, before and after the cache: an `Edit` transcript against
    /// the `Read` transcript `doc/console_rewrap_measurement.md` measured, at matched
    /// element counts.
    ///
    /// The `Read` row is the baseline both Edit columns are read against — it is the same
    /// corpus in both, and it moves only by measurement noise, which is what makes it a
    /// usable control for how much this run drifted.
    #[test]
    #[ignore = "a benchmark, not a correctness check — see doc/console_edit_diff_cost.md"]
    fn edit_card_frame_cost_table() {
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        println!(
            "\n| elems | cards | corpus           | uncached ms | cached ms |  saved ms | \
             saved µs/card | uncached vs Read | passes |"
        );
        println!(
            "|-------|-------|------------------|-------------|-----------|-----------|\
             ---------------|------------------|--------|"
        );
        for (elements, samples) in [(20, 40), (100, 40), (400, 20), (2_000, 8)] {
            let base = frames(elements, Fill::Read, samples, Cache::On);
            println!(
                "| {:>5} | {:>5} | {:<16} | {:>11.3} | {:>9.3} | {:>9} | {:>13} | {:>16} | {:>6} |",
                base.elements,
                base.cards,
                base.label,
                ms(frames(elements, Fill::Read, samples, Cache::Off).median),
                ms(base.median),
                "",
                "",
                "",
                base.passes,
            );
            let fills = Shape::ALL.map(Fill::Every).into_iter().chain([Fill::LargeOneIn(10)]);
            for fill in fills {
                let off = frames(elements, fill, samples, Cache::Off);
                let on = frames(elements, fill, samples, Cache::On);
                let saved = off.median.saturating_sub(on.median);
                println!(
                    "| {:>5} | {:>5} | {:<16} | {:>11.3} | {:>9.3} | {:>9.3} | {:>13.1} | \
                     {:>15.2}× | {:>6} |",
                    on.elements,
                    on.cards,
                    on.label,
                    ms(off.median),
                    ms(on.median),
                    ms(saved),
                    saved.as_secs_f64() * 1e6 / on.cards.max(1) as f64,
                    off.median.as_secs_f64() / base.median.as_secs_f64(),
                    on.passes,
                );
            }
        }
        println!();
    }
}

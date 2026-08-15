//! **What it costs to re-lay-out the transcript when its available width changes.**
//!
//! A measurement, not a feature. The written finding is `doc/console_rewrap_measurement.md`;
//! this module is the instrument that produced its numbers, kept in the tree so the numbers
//! can be re-taken on another machine or after an egui bump rather than believed.
//!
//! Two roadmaps are downstream of the answer. Posture's tween animates a margin token
//! 0 → 90 pt on **each** side, which changes the transcript's available width on **every
//! frame of the tween**; splitting a pane changes it the same way in one step. Both are
//! "the whole scrollback re-wraps", differing only in how many times.
//!
//! # What is real here, and what is not
//!
//! **Real:** the draw path. [`super::scrollback`] is called — the same function
//! `conversation_view::draw` calls — over a real [`Transcript`], inside a real
//! [`egui::ScrollArea`], on a real [`egui::Context`] with egui's own bundled fonts. Every
//! label goes through `egui::Label::layout_in_ui`, which builds its galley **before** the
//! `is_rect_visible` check (`egui-0.33.3/src/widgets/label.rs:278` vs `:282`), so an
//! off-screen element pays full layout exactly as an on-screen one does. Nothing here
//! approximates that.
//!
//! **Not real:** the *words*. The corpus is built from [`AgentEvent`]s of the shapes a live
//! session produces — human input, streamed-then-settled assistant prose, a tool card with
//! JSON arguments and clipped output, a run end — but the prose is written here rather than
//! captured. The captured fixtures in `fixtures/` are 11–77 lines, which is a handful of
//! elements; a four-hundred-element session has never been captured on this machine.
//! ⚠️ Every string carries its turn index, deliberately: identical text at one width is
//! **one** galley in epaint's cache however many elements draw it, so a corpus of repeated
//! paragraphs would have measured deduplication rather than layout.
//!
//! **Not measured at all:** tessellation and paint. [`egui::Context::run`] stops at shapes;
//! `Context::tessellate` is never called and no GPU is involved. The question is CPU text
//! layout, and this is the envelope that contains it. Nor is the diff a `tool_card` for an
//! `Edit` recomputes every frame ([`super::edit_diff`]) — the corpus is `Read` cards, and
//! that cost is real but is not a *wrapping* cost.
//!
//! # Running it
//!
//! ```text
//! cargo test --release -p organon-console --lib -- --ignored --nocapture rewrap
//! ```
//!
//! ⚠️ **`--release`, because the console ships release.** The workspace's `[profile.dev]`
//! is `opt-level = 1`, which the test profile inherits, and it reports figures about 25 %
//! high. The document records both.
//!
//! `#[ignore]` because it is seconds, not milliseconds, and the default suite is a
//! correctness gate. The one test here that is *not* ignored is
//! [`tests::width_is_part_of_the_galley_cache_key`], which is fast and pins the epaint
//! contract the whole document rests on — the same reason
//! `native/tests/egui_popup_contract.rs` exists.

use std::time::{Duration, Instant};

use super::{scrollback, ConversationPane, SurfaceImages};
use crate::agent_map::EventMapper;
use crate::approval::{approval_channel, DecisionMemory};
use crate::card_density::DensityMap;
use crate::conversation::{
    AgentEvent, MessageId, ResultDetail, RunOutcome, ToolId, Transcript,
};
use crate::posture::Form;
use crate::theme::Theme;
use std::collections::{HashMap, VecDeque};

/// The pane the console opens at, in points — `organon-console`'s default window is
/// 1100×720 and the conversation front-end fills it.
const PANE_W: f32 = 1100.0;
const PANE_H: f32 = 720.0;

/// The width this bench sweeps, in points: `PANE_W` → `PANE_W - SWEEP` and back.
///
/// ⚠️ **It is posture's desktop margin on ONE side, and posture claims that margin on both**
/// — so the real travel between the two ends is `2 × 90`, not this. The sweep is kept at the
/// range the figures in `doc/console_rewrap_measurement.md` were taken over, deliberately,
/// because **the finding is per-*change*, not per-magnitude**: egui's galley cache is keyed
/// on the wrap width, so a width that moves by one point is as total a miss as one that moves
/// by a hundred. Doubling this would lengthen the triangle and change nothing worth knowing.
const SWEEP: f32 = 90.0;

/// Frames run and thrown away before any timing is kept.
///
/// ⚠️ Not a ritual number. The first frame of a fresh [`egui::Context`] rasterises glyphs
/// into an empty font atlas and allocates every widget's memory for the first time; frames
/// two and three still settle the `ScrollArea`'s own state. Keeping them would report
/// warm-up as a result, which is one of two measurement traps this machine's log already
/// records the hard way.
const WARMUP: usize = 6;

// ---------------------------------------------------------------------------
// The corpus

/// Elements one synthetic turn contributes: human, prose, tool card, prose, run end.
const PER_TURN: usize = 5;

/// A transcript of roughly `elements` elements, built through the real fold.
///
/// Every string is unique to its turn — see the module doc's note about deduplication.
fn corpus(elements: usize) -> Transcript {
    let mut t = Transcript::new();
    t.apply(AgentEvent::SessionStarted { session_id: "rewrap-bench".into() });
    for turn in 0..elements.div_ceil(PER_TURN) {
        for event in one_turn(turn) {
            t.apply(event);
        }
    }
    t
}

/// The five events of one turn, at the shapes `agent_map` emits for a real session.
fn one_turn(turn: usize) -> Vec<AgentEvent> {
    let msg = |n: usize| MessageId(format!("msg_{turn}_{n}"));
    let tool = ToolId(format!("toolu_{turn}"));
    vec![
        AgentEvent::HumanInput { text: human_text(turn) },
        AgentEvent::AssistantMessage { message: msg(0), text: prose_text(turn) },
        AgentEvent::ToolCall {
            id: tool.clone(),
            name: "Read".into(),
            arguments: Some(arguments_text(turn)),
        },
        AgentEvent::ToolResult {
            id: tool,
            output: output_text(turn),
            is_error: false,
            detail: ResultDetail {
                file_path: Some(format!("C:\\work\\organon\\native\\src\\part_{turn}.rs")),
                lines: Some(120),
                total_lines: Some(2_400),
                start_line: Some(1),
            },
        },
        AgentEvent::AssistantMessage { message: msg(1), text: closing_text(turn) },
        AgentEvent::RunFinished { outcome: RunOutcome::Ok, detail: None },
    ]
    .into_iter()
    .take(PER_TURN + 1)
    .collect()
}

fn human_text(turn: usize) -> String {
    format!(
        "In turn {turn}: read the part of the render path that owns the surface table and \
         tell me whether the wrap width is decided there or one level up."
    )
}

/// ~600 characters — five or six wrapped rows at 1100 pt, which is what an ordinary
/// paragraph of an agent's reply measures.
fn prose_text(turn: usize) -> String {
    format!(
        "Looking at turn {turn}, the surface table is built where the pass is recorded rather \
         than where it is consumed, so the width the caller hands down is already the wrapped \
         width by the time anything reads it. That means the decision is one level up, in the \
         caller, and the table itself only ever sees a number it cannot influence. The \
         consequence worth stating is that a change of available width does not travel through \
         the table at all — it travels through the caller, which rebuilds the whole thing, and \
         the table's own cache key never learns that anything moved."
    )
}

fn arguments_text(turn: usize) -> String {
    format!(
        "{{\"file_path\": \"C:\\\\work\\\\organon\\\\native\\\\src\\\\part_{turn}.rs\", \
         \"offset\": {turn}, \"limit\": 120}}"
    )
}

/// Twelve lines, of which a card draws ten and counts the rest — the ordinary shape.
fn output_text(turn: usize) -> String {
    (0..12)
        .map(|line| {
            format!(
                "{:>5}\tlet surface_{turn}_{line} = table.entry(key).or_insert_with(Surface::new);",
                line + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn closing_text(turn: usize) -> String {
    format!(
        "So for turn {turn} the answer is the caller. I have left the table alone; if you want \
         the width pinned instead of inherited it belongs beside the pass record, not here."
    )
}

// ---------------------------------------------------------------------------
// The pane

/// A [`ConversationPane`] holding `transcript` and nothing else.
///
/// Built field-by-field rather than through [`ConversationPane::new`], which spawns a real
/// `claude.exe` and starts an MCP server on a real port. Every field this omits is one
/// [`scrollback`] does not read for a Human/Assistant/Tool/RunEnd body: no artifact and no
/// approval is in the corpus, so the two side maps stay empty and the two bodies drawn from
/// pane state are never reached.
///
/// 🚨 **`pub(super)`, and it is the one piece of bench scaffolding that is genuinely shared
/// rather than merely similar.** [`super::edit_diff_bench`] and [`super::tests`]'s diff-cache
/// contracts both build their panes here. That is deliberate: this function enumerates
/// **every field of [`ConversationPane`]**, so it stops compiling the day the struct gains
/// one — which is exactly what happened when `diffs` landed. A second copy would have to be
/// found and fixed separately every time, and the failure of *not* fixing it is a bench
/// measuring a pane that is not the pane.
///
/// ⚠️ The two `frame` drivers are deliberately **not** shared. They vary different things —
/// this module's takes a width and imposes an inset, `edit_diff_bench`'s takes a
/// `Cache` and imposes nothing — and folding them would give each measurement a parameter
/// it does not use and must remember to hold still.
pub(super) fn bench_pane(transcript: Transcript) -> ConversationPane {
    let (_gate, inbox) = approval_channel();
    ConversationPane {
        session: None,
        transcript,
        mapper: EventMapper::new(),
        theme_edit: None,
        failure: None,
        composer: String::new(),
        log: VecDeque::new(),
        pinned: false,
        want_focus: false,
        composer_height: 0.0,
        artifacts: HashMap::new(),
        diffs: HashMap::new(),
        // ⚠️ Empty, and `pinned` above is `false`, so **nothing settles and nothing
        // collapses**: both benches keep measuring the open card they have always measured,
        // and their published figures stay comparable across this change. A bench that
        // silently started measuring one-line rows would report a large speed-up that is
        // really a change of subject.
        density: DensityMap::default(),
        buttons: Vec::new(),
        sliders: Vec::new(),
        approvals: None,
        inbox,
        last_audit: None,
        waiting: HashMap::new(),
        memory: DecisionMemory::new(),
        models: Vec::new(),
        pending_model: None,
        // A bench pane draws; it never submits. An empty console catalog leaves the registry
        // holding only the view lane, and `NoDispatch` is the honest occupant of a lane
        // nothing in this file can reach.
        registry: super::Registry::new(&[]),
        local: Box::new(crate::mcp::NoDispatch),
        // The command panel is a band above the composer and neither bench measures it: the
        // composer is empty, so no line is a command line and nothing is drawn. `autorun` is
        // off for the same reason it is off in the product — a bench must not run commands.
        palette_selected: 0,
        palette_dismissed: false,
        composer_seen: String::new(),
        completion_held: false,
        history: std::collections::VecDeque::new(),
        history_at: None,
        want_caret: false,
        receipt: None,
        autorun: false,
        // The verbose list is off here for the same reason it is off in the product: the
        // primary panel is the one row, and a bench must measure what a person will see.
        verbose: false,
    }
}

/// One frame of the scrollback at `width`: how long it took, and how many **passes** egui
/// ran inside it.
///
/// ⚠️ The pass count is not decoration. [`egui::Context::run`] re-runs the whole ui when
/// something calls `request_discard`, up to `Options::max_passes`, and a frame that runs
/// twice lays the transcript out twice. Timing without counting would report a doubled
/// layout as a doubled *layout cost*, which is a different claim.
fn frame(
    ctx: &egui::Context,
    pane: &mut ConversationPane,
    images: &SurfaceImages,
    theme: &Theme,
    width: f32,
) -> (Duration, u8) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(PANE_W, PANE_H),
        )),
        ..Default::default()
    };
    let start = Instant::now();
    let out = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            // The inset, as posture will impose it: the transcript is given a column
            // narrower than the pane, and everything inside it wraps to that column.
            let size = egui::vec2(width, ui.available_height());
            ui.allocate_ui(size, |ui| {
                ui.set_width(width);
                // 🚨 **The TERMINAL form, and it must be.** This harness imposes the inset
                // itself, by narrowing the column above — so a desktop `Form` would apply it a
                // second time through `content_margin` and every measurement would be taken at
                // a width neither the caller nor the table names. Posture is not what this
                // bench varies; width is.
                let _ = scrollback(ui, pane, images, &Default::default(), theme, &Form::TERMINAL, &mut |_, _| {});
            });
        });
    });
    (start.elapsed(), out.platform_output.num_completed_passes as u8)
}

// ---------------------------------------------------------------------------
// The measurement

/// What one condition measured.
struct Reading {
    elements: usize,
    label: &'static str,
    samples: Vec<Duration>,
    /// Galleys epaint held at the end of the last frame — the cache's own account of how
    /// much of the scrollback it is carrying.
    galleys: usize,
    /// The most passes any kept frame ran. See [`frame`].
    passes: u8,
}

impl Reading {
    fn median(&self) -> Duration {
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    fn min(&self) -> Duration {
        *self.samples.iter().min().expect("a reading has samples")
    }

    fn max(&self) -> Duration {
        *self.samples.iter().max().expect("a reading has samples")
    }

    fn row(&self) -> String {
        format!(
            "| {:>5} | {:<9} | {:>9.3} | {:>9.3} | {:>9.3} | {:>3} | {:>7} | {:>6} |",
            self.elements,
            self.label,
            self.median().as_secs_f64() * 1e3,
            self.min().as_secs_f64() * 1e3,
            self.max().as_secs_f64() * 1e3,
            self.samples.len(),
            self.galleys,
            self.passes,
        )
    }
}

/// Run `WARMUP + samples` frames, keeping the last `samples`, with widths from `width`.
///
/// `width(frame_index)` decides the condition: a constant returns steady state, a walk
/// returns the tween.
fn measure(
    elements: usize,
    label: &'static str,
    samples: usize,
    width: impl Fn(usize) -> f32,
) -> Reading {
    let ctx = egui::Context::default();
    let images = SurfaceImages::new();
    let theme = Theme::organon();
    let mut pane = bench_pane(corpus(elements));
    let mut kept = Vec::with_capacity(samples);
    let mut passes = 0;
    for i in 0..WARMUP + samples {
        let (took, ran) = frame(&ctx, &mut pane, &images, &theme, width(i));
        if i >= WARMUP {
            kept.push(took);
            passes = passes.max(ran);
        }
    }
    let galleys = ctx.fonts(|f| f.num_galleys_in_cache());
    Reading { elements, label, samples: kept, galleys, passes }
}

/// Steady state: the width never moves, so every galley the scrollback asks for was already
/// in the cache from the frame before.
fn steady(elements: usize, samples: usize) -> Reading {
    measure(elements, "steady", samples, |_| PANE_W)
}

/// The tween: one point of inset per frame, 0 → 90 and back, forever.
///
/// A triangle rather than a ramp because a tween that only ever narrows would run out of
/// widths; and because the return leg is the honest test of whether the cache remembers a
/// width it saw ninety frames ago. It does not — `flush_cache` keeps only what this frame
/// used (`epaint-0.33.3/src/text/fonts.rs:1067`).
fn animating(elements: usize, samples: usize) -> Reading {
    measure(elements, "animating", samples, |i| {
        let period = (SWEEP as usize) * 2;
        let phase = i % period;
        let inset = if phase <= SWEEP as usize { phase } else { period - phase };
        PANE_W - inset as f32
    })
}

/// **One step, which is what splitting a pane actually does** — and what "snap the width
/// once at the end" would reduce a tween to.
///
/// Returns the median frame the width moved on, and the median of the frames after it at
/// the new constant width. The second number is the one the option turns on: if a single
/// change leaves a lasting cost, snapping buys nothing.
///
/// ⚠️ A step can only happen **once** per context — after it, the new width is the standing
/// one — so `repeats` is a count of whole fresh experiments, not of samples within one.
/// That is why this is separate from [`measure`] rather than a third `width` closure.
fn one_step(elements: usize, repeats: usize) -> (Duration, Duration) {
    let mut moves = Vec::with_capacity(repeats);
    let mut afters = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let ctx = egui::Context::default();
        let images = SurfaceImages::new();
        let theme = Theme::organon();
        let mut pane = bench_pane(corpus(elements));
        for _ in 0..WARMUP {
            frame(&ctx, &mut pane, &images, &theme, PANE_W);
        }
        moves.push(frame(&ctx, &mut pane, &images, &theme, PANE_W - SWEEP).0);
        let mut after: Vec<Duration> = (0..10)
            .map(|_| frame(&ctx, &mut pane, &images, &theme, PANE_W - SWEEP).0)
            .collect();
        after.sort_unstable();
        afters.push(after[after.len() / 2]);
    }
    moves.sort_unstable();
    afters.sort_unstable();
    (moves[moves.len() / 2], afters[afters.len() / 2])
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 🚨 **The fact the whole document rests on: the wrap width is part of the galley
    /// cache key, so a width that moves is a 100 % miss.**
    ///
    /// Pinned by driving epaint's real cache rather than by reading it, because this is a
    /// property of a pinned dependency that a version bump can change silently — the same
    /// argument `native/tests/egui_popup_contract.rs` was written under.
    ///
    /// Three claims, in one pass so the per-frame flush cannot interfere:
    ///
    /// 1. Two widths a point apart are two entries.
    /// 2. Two widths that **round to the same integer** are one — `layout_internal` rounds
    ///    `max_width` before hashing (`fonts.rs:879`), which is why a sub-pixel tween does
    ///    not thrash and a whole-pixel one does.
    /// 3. The same width twice is one entry, i.e. the cache is doing its job at all.
    #[test]
    fn width_is_part_of_the_galley_cache_key() {
        let ctx = egui::Context::default();
        let text = "a paragraph long enough that the wrap width can actually change where \
                    its rows break, which is the whole point of asking";
        let job = |w: f32| {
            egui::text::LayoutJob::simple(
                text.to_owned(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
                w,
            )
        };
        let mut counts = Vec::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts_mut(|f| {
                let _ = f.layout_job(job(400.0));
                counts.push(f.num_galleys_in_cache());
                let _ = f.layout_job(job(400.0));
                counts.push(f.num_galleys_in_cache());
                let _ = f.layout_job(job(400.4));
                counts.push(f.num_galleys_in_cache());
                let _ = f.layout_job(job(401.0));
                counts.push(f.num_galleys_in_cache());
            });
        });
        let [first, repeat, rounded, moved] = counts[..] else {
            unreachable!("four probes")
        };
        assert_eq!(repeat, first, "the same job twice must be one cache entry, not two");
        assert_eq!(
            rounded, first,
            "400.4 rounds to 400 before hashing, so it must reuse the entry — this is what \
             keeps a sub-pixel layout wobble from re-wrapping the world"
        );
        assert_eq!(
            moved,
            first + 1,
            "a whole point of width is a different key, so every galley misses when the \
             width moves: {counts:?}"
        );
    }

    /// The corpus is what it claims to be: real elements, all four bodies, unique text.
    #[test]
    fn the_corpus_is_a_transcript_of_unique_elements() {
        let t = corpus(100);
        assert!(t.len() >= 100, "asked for 100 elements, got {}", t.len());
        let prose: Vec<&str> =
            t.elements().iter().filter_map(|e| e.assistant()).map(|a| a.text.as_str()).collect();
        assert!(prose.len() > 20, "the corpus must be mostly prose: {}", prose.len());
        let mut unique: Vec<&str> = prose.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            prose.len(),
            "repeated text would collapse in the galley cache and understate the cost"
        );
        assert!(
            t.elements().iter().any(|e| e.tool().is_some()),
            "a session without tool cards is not the session anyone runs"
        );
    }

    /// A frame really does lay the whole scrollback out, off-screen elements included.
    ///
    /// The proof is the cache's own count: a 720 pt viewport shows perhaps a dozen
    /// elements, so if egui culled, the galley count would not move with the transcript.
    #[test]
    fn the_whole_scrollback_is_laid_out_not_just_the_visible_slice() {
        let small = steady(20, 1).galleys;
        let large = steady(200, 1).galleys;
        assert!(
            large > small * 4,
            "ten times the transcript must be far more galleys — otherwise something is \
             culling and this whole measurement is about the wrong thing: {small} vs {large}"
        );
    }

    /// **The measurement.** Ignored: seconds, and the default suite is a correctness gate.
    ///
    /// `cargo test -p organon-console --lib -- --ignored --nocapture rewrap`
    #[test]
    #[ignore = "a benchmark, not a correctness check — see doc/console_rewrap_measurement.md"]
    fn rewrap_cost_table() {
        println!(
            "\n| elems | condition |  med (ms) |  min (ms) |  max (ms) |   n | galleys | passes |"
        );
        println!(
            "|-------|-----------|-----------|-----------|-----------|-----|---------|--------|"
        );
        for (elements, samples) in [(20, 60), (100, 60), (400, 40), (2_000, 20), (10_000, 8)] {
            let s = steady(elements, samples);
            let a = animating(elements, samples);
            println!("{}", s.row());
            println!("{}", a.row());
            println!(
                "|       | ratio     | {:>9.2} |           |           |     |         |        |",
                a.median().as_secs_f64() / s.median().as_secs_f64()
            );
        }
        println!(
            "\n| elems | the one frame a width change lands on | every frame after it | runs |"
        );
        println!("|-------|--------------------------------------|----------------------|------|");
        for (elements, repeats) in [(20, 9), (100, 9), (400, 9), (2_000, 5), (10_000, 3)] {
            let (moved, after) = one_step(elements, repeats);
            println!(
                "| {:>5} | {:>34.3} ms | {:>17.3} ms | {:>4} |",
                elements,
                moved.as_secs_f64() * 1e3,
                after.as_secs_f64() * 1e3,
                repeats
            );
        }
        println!();
    }
}

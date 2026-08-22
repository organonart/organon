//! Painting the training strip and the run shelf (#147 Tier 4).
//!
//! The thinking lives in `organon_core::train`: the SSE framing, the chunked framing, the
//! fold, and the state machine whose whole point is that *"nothing is training"*, *"I cannot
//! reach the Studio"* and *"my key is wrong"* are three different sentences. **This module
//! only draws.** Same split as `mind_viz`: the pure part is unit-tested where it lives, and
//! the `paint_*` functions here are thin egui over already-computed numbers.
//!
//! 🚨 **Nothing here may render a reachable Studio as "connected".** T1 established that
//! `GET /api/health` is unauthenticated, so a green probe proves only that the app is
//! running. `organon_core::train` answers that by never probing — the first *authenticated*
//! call is what produces [`LinkState::Idle`] — and this module's job is to keep the
//! distinction visible: [`state_dot`] takes its colour from [`Severity`], and the caller
//! shows [`LinkState::asserts`] on hover so the claim behind the colour is one pointer-hover
//! away rather than something a viewer has to infer.
//!
//! 📌 **Absence is drawn quietly.** The Studio is off most of the time on the machine this
//! was written for; [`Severity::Quiet`] is a dim grey, not a red, so the one colour that
//! should mean something keeps meaning it.
//!
//! ⚠️ **The loss curve is drawn on a log y axis and the learning rate is not**, and neither
//! choice is neutral. Training loss falls across orders of magnitude and a linear axis
//! flattens the whole interesting tail into the bottom pixel row; a learning-rate schedule
//! is the shape of the schedule and reads correctly linear. Both are display choices, so
//! both are named here rather than left in the arithmetic.
//!
//! No `Shared` field, no `LAYOUT_VERSION` movement — this is an editor-side readout, exactly
//! as `MIND_ARCHITECTURE.md` §5 routes a new analytics widget.

use organon_core::train::{LinkState, RunSummary, Severity, TrainingStrip};

// ── Palette ──────────────────────────────────────────────────────────────────
// Deliberately the `mind_viz` family so the training strip reads as part of the same
// instrument rather than as a bolted-on dashboard.

const BG: egui::Color32 = egui::Color32::from_rgb(10, 12, 20);
const GRID: egui::Color32 = egui::Color32::from_rgb(34, 40, 56);
/// Loss: the amber accent — the number the eye should go to.
pub const LOSS_COL: egui::Color32 = egui::Color32::from_rgb(255, 181, 71);
/// Learning rate: the cool companion, as `mind_viz`'s MLP rail is to its depth profile.
pub const LR_COL: egui::Color32 = egui::Color32::from_rgb(120, 150, 235);
/// Gradient norm: `mind_viz`'s entropy amber-red, because a spiking gradient is the same
/// kind of "look at this" as a spiking entropy.
pub const GRAD_COL: egui::Color32 = egui::Color32::from_rgb(214, 120, 190);

/// Quiet: nothing is wrong and nothing is happening.
const QUIET_COL: egui::Color32 = egui::Color32::from_rgb(120, 128, 145);
/// Active: a run is delivering.
const ACTIVE_COL: egui::Color32 = egui::Color32::from_rgb(96, 206, 132);
/// Attention: a person can fix this.
const ATTENTION_COL: egui::Color32 = egui::Color32::from_rgb(235, 150, 70);

/// The colour a link state reads as.
///
/// 📌 **Unreachable is grey, not red.** The Studio not running is the normal case, and a red
/// that fires every day is a red nobody reads. Only [`Severity::Attention`] — a rejected key,
/// a `5xx`, an unrecognised body — is warm.
pub fn state_colour(state: &LinkState) -> egui::Color32 {
    match state.severity() {
        Severity::Quiet => QUIET_COL,
        Severity::Active => ACTIVE_COL,
        Severity::Attention => ATTENTION_COL,
    }
}

/// The one-glyph status mark: `●` for a live run, `○` for everything else.
///
/// ⚠️ A filled dot means **a run is under way**, not "connected" — those are different
/// claims and only [`LinkState::Live`] supports the first. An idle-but-authenticated link
/// gets a hollow dot in the good-news grey, which is honest about both halves.
pub fn state_dot(state: &LinkState) -> &'static str {
    if matches!(state, LinkState::Live) {
        "●"
    } else {
        "○"
    }
}

/// The compact numbers row: `step 40/60 · loss 1.284 · lr 1.8e-4 · ‖g‖ 0.94`.
///
/// Absent numbers are simply left out rather than shown as `0`, because a zero loss and an
/// unreported loss are very different things and only one of them is good news.
pub fn numbers_line(strip: &TrainingStrip) -> String {
    let mut parts: Vec<String> = Vec::new();
    match (strip.step, strip.total_steps) {
        (Some(s), Some(t)) if t > 0 => parts.push(format!("step {s}/{t}")),
        (Some(s), _) => parts.push(format!("step {s}")),
        _ => {}
    }
    if let Some(l) = strip.loss.filter(|v| v.is_finite()) {
        parts.push(format!("loss {l:.4}"));
    }
    if let Some(lr) = strip.lr.filter(|v| v.is_finite()) {
        parts.push(format!("lr {lr:.3e}"));
    }
    if let Some(g) = strip.grad_norm.filter(|v| v.is_finite()) {
        parts.push(format!("‖g‖ {g:.3}"));
    }
    if let Some(e) = strip.epoch.filter(|v| v.is_finite()) {
        parts.push(format!("epoch {e:.2}"));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" · ")
    }
}

/// 🚨 The tell for a payload field-name guess that missed: events are arriving and every
/// number is blank. Returns the sentence to show, or `None` when there is nothing odd.
///
/// The SSE `progress` payload's field names have never been read off a running Studio (see
/// `organon_core::train`'s module doc). If they are wrong, `serde` still succeeds — every
/// field is optional — and the strip fills with nothing. That is indistinguishable from a
/// quiet trainer unless somebody says so, which is what this is for.
pub fn payload_warning(strip: &TrainingStrip) -> Option<String> {
    if strip.events_seen >= 3 && strip.step.is_none() && strip.loss.is_none() {
        return Some(format!(
            "{} progress events arrived and none carried a number this build recognises — \
             the field names in the SSE payload are probably not the ones assumed.",
            strip.events_seen
        ));
    }
    None
}

/// Draw an `(x, y)` curve into `rect`, auto-ranged on both axes.
///
/// `log_y` compresses the y axis by `log10` — see the module doc for why the loss uses it
/// and the learning rate does not. Fewer than two points draws the frame and nothing else,
/// which is the honest picture of "one sample so far".
pub fn paint_curve(
    p: &egui::Painter,
    rect: egui::Rect,
    pts: &[(u64, f64)],
    colour: egui::Color32,
    log_y: bool,
) {
    p.rect_filled(rect, 2.0, BG);
    let inner = rect.shrink(2.0);
    // Two horizontal rules, so a flat line still reads as flat against something.
    for f in [0.33_f32, 0.66] {
        let y = inner.bottom() - inner.height() * f;
        p.line_segment(
            [egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)],
            egui::Stroke::new(1.0_f32, GRID),
        );
    }
    if pts.len() < 2 {
        return;
    }
    let ys: Vec<f32> = pts
        .iter()
        .map(|(_, v)| {
            if log_y {
                // ⚠️ `max(1e-9)` rather than a filter: a loss of exactly 0 is real (and
                // rare), and dropping the point would put a gap in the curve where the model
                // did best. Clamping puts it at the floor, which is where it belongs.
                (v.max(1e-9)).log10() as f32
            } else {
                *v as f32
            }
        })
        .collect();
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for y in &ys {
        if y.is_finite() {
            lo = lo.min(*y);
            hi = hi.max(*y);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return;
    }
    // A perfectly flat series would divide by zero; give it a band and draw it centred.
    if (hi - lo).abs() < f32::EPSILON {
        lo -= 0.5;
        hi += 0.5;
    }
    let x0 = pts[0].0 as f32;
    let x1 = pts[pts.len() - 1].0 as f32;
    let span = (x1 - x0).max(1.0);
    let at = |i: usize| {
        let t = (pts[i].0 as f32 - x0) / span;
        let u = (ys[i] - lo) / (hi - lo);
        egui::pos2(
            inner.left() + inner.width() * t.clamp(0.0, 1.0),
            inner.bottom() - inner.height() * u.clamp(0.0, 1.0),
        )
    };
    for i in 1..pts.len() {
        p.line_segment([at(i - 1), at(i)], egui::Stroke::new(1.5_f32, colour));
    }
    p.circle_filled(at(pts.len() - 1), 2.2, colour);
}

/// Draw a bare value series with no x axis — the shelf's `loss_sparkline`, which the Studio
/// serves as values alone.
pub fn paint_sparkline(
    p: &egui::Painter,
    rect: egui::Rect,
    values: &[f64],
    colour: egui::Color32,
) {
    let pts: Vec<(u64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (i as u64, *v))
        .collect();
    paint_curve(p, rect, &pts, colour, true);
}

/// A thin progress bar, `0.0..=1.0`.
pub fn paint_progress(p: &egui::Painter, rect: egui::Rect, t: f32, colour: egui::Color32) {
    p.rect_filled(rect, 1.5, GRID);
    let t = t.clamp(0.0, 1.0);
    if t > 0.0 {
        let mut filled = rect;
        filled.max.x = rect.left() + rect.width() * t;
        p.rect_filled(filled, 1.5, colour);
    }
}

/// A shelf row's one-line summary: `gemma-3-4b · alpaca · 60 steps · loss 0.912 · 1m 35s`.
///
/// Missing fields are dropped rather than filled with placeholders — a run the Studio
/// described sparsely should read as sparse, not as a run with an empty dataset name.
pub fn run_line(run: &RunSummary) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = run.model_name.as_deref().filter(|s| !s.is_empty()) {
        parts.push(m.to_string());
    }
    if let Some(d) = run.dataset_name.as_deref().filter(|s| !s.is_empty()) {
        parts.push(d.to_string());
    }
    match (run.final_step, run.total_steps) {
        (Some(f), Some(t)) if t > 0 && f < t => parts.push(format!("{f}/{t} steps")),
        (_, Some(t)) if t > 0 => parts.push(format!("{t} steps")),
        (Some(f), _) => parts.push(format!("{f} steps")),
        _ => {}
    }
    if let Some(l) = run.final_loss.filter(|v| v.is_finite()) {
        parts.push(format!("loss {l:.3}"));
    }
    if run.duration_seconds.is_some() {
        parts.push(run.duration_label());
    }
    if parts.is_empty() {
        // A run with no describable properties still has an identity.
        return if run.id.is_empty() {
            "(a run the Studio described with nothing)".to_string()
        } else {
            run.id.clone()
        };
    }
    parts.join(" · ")
}

/// The colour a shelf row's status word reads as.
pub fn run_colour(run: &RunSummary) -> egui::Color32 {
    if run.is_failed() {
        ATTENTION_COL
    } else if run.is_running() {
        ACTIVE_COL
    } else if run.is_complete() {
        LOSS_COL
    } else {
        QUIET_COL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use organon_core::train::TrainingStrip;

    #[test]
    fn absence_is_grey_and_a_bad_key_is_warm() {
        // 📌 The Studio being off is the normal case, so it must not paint like an alarm.
        assert_eq!(
            state_colour(&LinkState::Unreachable {
                authority: "127.0.0.1:8888".into(),
                detail: "refused".into()
            }),
            QUIET_COL
        );
        assert_eq!(state_colour(&LinkState::NotConfigured), QUIET_COL);
        assert_eq!(state_colour(&LinkState::Idle), QUIET_COL);
        assert_eq!(state_colour(&LinkState::Live), ACTIVE_COL);
        assert_eq!(
            state_colour(&LinkState::Unauthorized { status: 401 }),
            ATTENTION_COL
        );
        assert_ne!(QUIET_COL, ATTENTION_COL);
    }

    #[test]
    fn only_a_live_run_gets_a_filled_dot() {
        // ⚠️ A filled dot must not come to mean "connected" — that is the T1 trap in
        // pictorial form.
        assert_eq!(state_dot(&LinkState::Live), "●");
        for s in [
            LinkState::Idle,
            LinkState::Unknown,
            LinkState::NotConfigured,
            LinkState::Unauthorized { status: 401 },
        ] {
            assert_eq!(state_dot(&s), "○", "{s:?} is not a run in progress");
        }
    }

    #[test]
    fn an_absent_number_is_omitted_rather_than_shown_as_zero() {
        let mut s = TrainingStrip::new();
        assert_eq!(numbers_line(&s), "—");
        s.step = Some(40);
        s.total_steps = Some(60);
        assert_eq!(numbers_line(&s), "step 40/60");
        s.loss = Some(1.2345);
        assert!(numbers_line(&s).contains("loss 1.2345"));
        assert!(!numbers_line(&s).contains("lr "), "an unreported lr is absent");
    }

    #[test]
    fn events_with_no_recognised_numbers_are_called_out() {
        // 🚨 The tell for a wrong field-name guess. Without this it looks like a quiet run.
        let mut s = TrainingStrip::new();
        assert!(payload_warning(&s).is_none());
        s.events_seen = 5;
        let w = payload_warning(&s).expect("five blank events must be reported");
        assert!(w.contains("field names"));
        s.step = Some(1);
        assert!(
            payload_warning(&s).is_none(),
            "one recognised number clears it"
        );
    }

    #[test]
    fn a_sparse_run_reads_as_sparse() {
        let bare = RunSummary {
            id: "run-7".into(),
            ..Default::default()
        };
        assert_eq!(run_line(&bare), "run-7");
        let full = RunSummary {
            id: "run-8".into(),
            model_name: Some("gemma-3-4b".into()),
            dataset_name: Some("alpaca".into()),
            total_steps: Some(60),
            final_step: Some(60),
            final_loss: Some(0.9123),
            duration_seconds: Some(95.0),
            ..Default::default()
        };
        let line = run_line(&full);
        assert!(line.contains("gemma-3-4b"), "{line}");
        assert!(line.contains("60 steps"), "{line}");
        assert!(line.contains("loss 0.912"), "{line}");
        assert!(line.contains("1m 35s"), "{line}");
    }

    #[test]
    fn an_unfinished_run_shows_both_step_counts() {
        let partial = RunSummary {
            final_step: Some(12),
            total_steps: Some(60),
            ..Default::default()
        };
        assert!(run_line(&partial).contains("12/60 steps"));
    }

    #[test]
    fn a_failed_run_is_the_only_warm_row() {
        let mk = |s: &str| RunSummary {
            status: s.into(),
            ..Default::default()
        };
        assert_eq!(run_colour(&mk("failed")), ATTENTION_COL);
        assert_eq!(run_colour(&mk("running")), ACTIVE_COL);
        assert_eq!(run_colour(&mk("complete")), LOSS_COL);
        assert_eq!(run_colour(&mk("who knows")), QUIET_COL);
    }
}

//! Mind-dashboard paint helpers (#482 Tier 1 — the LLM's oscilloscope).
//!
//! The Audio tab feels *alive* while sound plays — level meter, log spectrum, RTA,
//! oscilloscope. This is the same idea for a running model: a compact grid of little
//! widgets that light up per token, driven **purely by the existing `MindFrame`**
//! streamed into the activation ring (`mind_ring.rs`) — no writer change, no ring
//! layout change, no `Shared` change. The editor opens its own `MindRingReader`
//! (mmap reads are non-destructive) and paints these widgets on `UiTab::Mind`,
//! modeled one-for-one on how `lib.rs` paints `AudioViz` (peak-hold/decay caps,
//! scrolling scope, heat strip).
//!
//! Split by concern the way the audio meters are: the display state + its update
//! math (`MindViz::observe`, peak-hold decay, auto-gain, scroll, tokens/sec) is
//! **pure and unit-tested**; the `paint_*` methods are thin egui draws over the
//! already-computed buffers. Cross-model by construction — everything normalizes
//! against a running auto-gain, so any `.gguf` (whatever its layer/head count)
//! fills the widgets.
//!
//! **Default-inert:** no writer / not streaming → `observe(None, …)` decays every
//! buffer to a flat idle state (zeros), exactly like the Audio tab with silence.
//! Adds no `Shared` field and no `LAYOUT_VERSION` change (it reads the mind ring).
//!
//! **#482 Tier 2** adds the two honest headliners on top of the same reader: the
//! **top-k next-token bars** (the actual softmax over decoded candidates) and the
//! **heartbeat** — dual scrolling traces of the measured next-token `entropy` +
//! `confidence` (falling back to the Tier-1 effort proxy until stats arrive). These
//! read the new `MindFrame` fields (`mind_ring.rs`); still no `Shared` change.
//!
//! **#482 Tier 3** is the instrument finish: a consistent **provenance-tag** system
//! (`Provenance` + `provenance_row` — the papers' measured `=` / derived `~` / proxy `?`
//! mark, hover-explained) on every widget, and a **context / KV fuel gauge**
//! (`paint_context`, reading the new `ctx_used` / `ctx_total` frame fields). The
//! compact/expanded layout toggle lives in `lib.rs`.

use crate::mind_ring::{snippet_str, MindFrame, MR_MAX_HEADS, MR_MAX_LAYERS, MR_TOPK};

/// Scrolling-scope history length (one sample per generated token).
pub const SCOPE_CAP: usize = 240;
/// Peak-hold cap fall rate (fraction of full scale per second) — matches the
/// Audio spectrum's falling-cap feel.
const PEAK_FALL: f32 = 0.9;
/// Auto-gain decay (fraction per second): rises instantly to the loudest layer,
/// then relaxes so a quieter passage re-fills the widgets (spectrum-analyzer AGC).
const GAIN_FALL: f32 = 0.4;
/// Tokens/sec smoothing (EMA weight on each fresh inter-token interval).
const TPS_ALPHA: f32 = 0.35;
/// No new token for this long ⇒ the stream is idle; widgets settle to flat, tps → 0.
const IDLE_SECS: f64 = 0.75;
/// Idle decay of the live bars toward zero (fraction per second).
const IDLE_FALL: f32 = 2.5;
/// Floor for the auto-gain so a silent/zeroed ring never divides by ~0.
const GAIN_EPS: f32 = 1.0e-4;

// ── Pure helpers (unit-tested) ──────────────────────────────────────────────

/// One EMA step: pull `prev` toward `sample` by `alpha` (0..1).
pub fn ema(prev: f32, sample: f32, alpha: f32) -> f32 {
    prev + (sample - prev) * alpha.clamp(0.0, 1.0)
}

/// Peak-hold with a falling cap: jump up to `cur`, else decay toward it at
/// `fall`/sec (never below `cur`, never below 0). The Audio panel's cap look.
pub fn decay_peak(peak: f32, cur: f32, fall: f32, dt: f32) -> f32 {
    if cur >= peak {
        cur
    } else {
        (peak - fall * dt).max(cur).max(0.0)
    }
}

/// Auto-gain tracker: rise instantly to `observed`, else decay *relatively* (so it
/// scales across models of any magnitude), floored at `GAIN_EPS`.
pub fn track_gain(gain: f32, observed: f32, fall: f32, dt: f32) -> f32 {
    if observed >= gain {
        observed.max(GAIN_EPS)
    } else {
        (gain * (1.0 - fall * dt)).max(observed).max(GAIN_EPS)
    }
}

/// Aggregate activity of a frame = mean residual/activation norm over its active
/// layers (the "effort" the oscilloscope traces). 0 for an empty frame.
pub fn frame_activity(f: &MindFrame) -> f32 {
    let n = (f.n_layers as usize).min(MR_MAX_LAYERS);
    if n == 0 {
        return 0.0;
    }
    let sum: f32 = (0..n).map(|l| f.layer_norm[l].max(0.0)).sum();
    sum / n as f32
}

/// The loudest single scalar in a frame (over layer norms + MLP activity), used to
/// drive the auto-gain. 0 for an empty frame.
pub fn frame_max(f: &MindFrame) -> f32 {
    let n = (f.n_layers as usize).min(MR_MAX_LAYERS);
    let mut m = 0.0f32;
    for l in 0..n {
        m = m.max(f.layer_norm[l].abs()).max(f.mlp_act[l].abs());
    }
    m
}

/// Push a newest sample onto the tail of a bounded scroll buffer (oldest at front).
pub fn push_scroll(buf: &mut Vec<f32>, sample: f32, cap: usize) {
    buf.push(sample);
    if buf.len() > cap {
        let excess = buf.len() - cap;
        buf.drain(0..excess);
    }
}

// ── Display state ───────────────────────────────────────────────────────────

/// Editor-side (GUI-thread) display state for the Mind dashboard — the peak-hold
/// caps, scrolling scope history, auto-gain and tokens/sec estimate. Held in the
/// editor's `PresetUi`; fed one `MindFrame` per repaint via [`MindViz::observe`].
#[derive(Default)]
pub struct MindViz {
    /// Active layer / head counts from the latest frame (0 = idle).
    pub n_layers: usize,
    pub n_heads: usize,
    /// The latest generated token index (for the counter readout).
    pub token_index: u32,
    /// Per-layer normalized (0..1) current activation norm + its peak-hold cap.
    pub depth: Vec<f32>,
    pub depth_peaks: Vec<f32>,
    /// Per-layer normalized MLP activity + peak-hold cap.
    pub mlp: Vec<f32>,
    pub mlp_peaks: Vec<f32>,
    /// Row-major `[l * n_heads + h]` normalized attention summary (the heat strip).
    pub heat: Vec<f32>,
    /// #522 Tier 1 — the latest frame's provenance bits (`FLAG_RESID_MEASURED` /
    /// `FLAG_MLP_MEASURED`), so the depth + MLP widgets can show `=` when the writer
    /// tapped real tensors and `?` when it shaped the entropy proxy.
    ///
    /// Read from the frame rather than inferred: a reader cannot tell a measured depth
    /// profile from a shaped one by looking at the numbers, and guessing would defeat
    /// the point of having a provenance glyph at all. 0 while idle — no frame, no claim.
    pub prov_flags: u32,
    /// Scrolling per-token aggregate-effort trace (0..1), newest at the tail.
    pub effort: Vec<f32>,
    // ── #482 Tier 2 — the honest per-token stats ─────────────────────────────
    /// Scrolling per-token next-token softmax **entropy** (0..1), newest at tail.
    pub entropy_hist: Vec<f32>,
    /// Scrolling per-token top-token **confidence** (0..1), newest at tail.
    pub conf_hist: Vec<f32>,
    /// Current top-k candidate probabilities (descending) + their decoded, display-
    /// sanitized snippets, parallel; `topk_count` valid. Cleared when idle.
    pub topk_prob: Vec<f32>,
    pub topk_text: Vec<String>,
    pub topk_count: usize,
    // ── #482 Tier 3 — the context / KV fuel gauge ────────────────────────────
    /// KV-cache occupancy (prompt + generated) and the active window size. Held
    /// across idle (the window stays full after a run); zeroed when no stream.
    pub ctx_used: u32,
    pub ctx_total: u32,
    /// Running auto-gain (loudest scalar seen, decaying) — the normalizer.
    pub gain: f32,
    /// Smoothed tokens/sec ("tempo of thought").
    pub tps: f32,
    /// True while fresh tokens are arriving (drives the "live" light + repaint).
    pub active: bool,
    /// The last frame `seq` observed (new-token edge detector).
    last_seq: u32,
    /// Wall-clock (egui `input.time`) at the last observed new token.
    last_stamp: f64,
    /// Have we observed at least one frame this stream?
    seeded: bool,
}

impl MindViz {
    /// Fold the latest ring frame into the display state. `now` = egui `input.time`
    /// (seconds), `dt` = frame delta (seconds). `frame = None` (no writer / stale
    /// reader) decays everything toward a flat idle state.
    ///
    /// One sample is pushed to the effort scope **per new token** (seq edge), and
    /// tokens/sec is measured from the wall-clock gap between those edges — so both
    /// track the model's real cadence, not the editor's repaint rate.
    pub fn observe(&mut self, frame: Option<&MindFrame>, now: f64, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        // Auto-gain always relaxes; a present frame re-raises it below.
        self.gain = track_gain(self.gain, 0.0, GAIN_FALL, dt);

        let Some(f) = frame else {
            self.idle_decay(dt);
            self.clear_topk();
            // No stream at all → the fuel gauge reads empty (idle-after-a-run holds it,
            // handled in the streaming-lapsed branch below).
            self.ctx_used = 0;
            self.ctx_total = 0;
            self.active = false;
            self.tps = ema(self.tps, 0.0, 0.25);
            return;
        };

        let n = (f.n_layers as usize).min(MR_MAX_LAYERS);
        let h = (f.n_heads as usize).min(MR_MAX_HEADS);
        self.n_layers = n;
        self.n_heads = h;
        self.token_index = f.token_index;
        // #522 Tier 1 — carry the writer's provenance claim forward verbatim.
        self.prov_flags = f.flags;
        self.gain = track_gain(self.gain, frame_max(f), GAIN_FALL, dt);
        let g = self.gain.max(GAIN_EPS);

        // New-token edge: advance the scope + tokens/sec.
        let new_token = !self.seeded || f.seq != self.last_seq;
        if new_token {
            if self.seeded {
                let gap = (now - self.last_stamp).max(1.0e-4);
                self.tps = ema(self.tps, (1.0 / gap) as f32, TPS_ALPHA);
            }
            let act = (frame_activity(f) / g).clamp(0.0, 1.0);
            push_scroll(&mut self.effort, act, SCOPE_CAP);
            // #482 Tier 2 — the measured heartbeat: one entropy + confidence sample per
            // token (the writers set these; 0 on a stream that doesn't).
            push_scroll(&mut self.entropy_hist, f.entropy.clamp(0.0, 1.0), SCOPE_CAP);
            push_scroll(&mut self.conf_hist, f.confidence.clamp(0.0, 1.0), SCOPE_CAP);
            self.last_seq = f.seq;
            self.last_stamp = now;
            self.seeded = true;
        }

        let streaming = new_token || (now - self.last_stamp) <= IDLE_SECS;
        self.active = streaming;

        resize_to(&mut self.depth, n);
        resize_to(&mut self.depth_peaks, n);
        resize_to(&mut self.mlp, n);
        resize_to(&mut self.mlp_peaks, n);
        resize_to(&mut self.heat, n * h);

        if streaming {
            for l in 0..n {
                let dv = (f.layer_norm[l].max(0.0) / g).clamp(0.0, 1.0);
                self.depth[l] = dv;
                self.depth_peaks[l] = decay_peak(self.depth_peaks[l], dv, PEAK_FALL, dt);
                let mv = (f.mlp_act[l].max(0.0) / g).clamp(0.0, 1.0);
                self.mlp[l] = mv;
                self.mlp_peaks[l] = decay_peak(self.mlp_peaks[l], mv, PEAK_FALL, dt);
                for hd in 0..h {
                    let raw = f.head_summ[l * MR_MAX_HEADS + hd].max(0.0);
                    self.heat[l * h + hd] = (raw / g).clamp(0.0, 1.0);
                }
            }
            // #482 Tier 2 — refresh the top-k bars from the current frame (the same
            // distribution holds between tokens, so this is idempotent per token).
            let kc = (f.topk_count as usize).min(MR_TOPK);
            self.topk_count = kc;
            self.topk_prob.clear();
            self.topk_text.clear();
            for r in 0..kc {
                self.topk_prob.push(f.topk_prob[r].clamp(0.0, 1.0));
                self.topk_text
                    .push(sanitize_snippet(&snippet_str(&f.topk_text[r])));
            }
            // #482 Tier 3 — the KV fuel gauge (measured counts from the runtime).
            self.ctx_used = f.ctx_used;
            self.ctx_total = f.ctx_total;
        } else {
            // Fresh-token window lapsed: settle to flat, like meters after silence.
            self.idle_decay(dt);
            self.clear_topk();
            self.tps = ema(self.tps, 0.0, 0.25);
        }
    }

    /// Empty the top-k bars (no current next-token distribution to show).
    fn clear_topk(&mut self) {
        self.topk_count = 0;
        self.topk_prob.clear();
        self.topk_text.clear();
    }

    /// Does the live stream carry the #482 Tier 2 measured stats (entropy / confidence)?
    /// Decides whether the heartbeat shows the honest entropy+confidence traces (`=`) or
    /// falls back to the Tier-1 aggregate-effort proxy (`?`).
    pub fn has_stats(&self) -> bool {
        self.entropy_hist.iter().any(|&x| x > 0.0) || self.conf_hist.iter().any(|&x| x > 0.0)
    }

    /// #522 Tier 1 — provenance of the per-layer **residual** widget: `Measured` once a
    /// writer has tapped real `l_out-{N}` tensors, `Proxy` otherwise.
    ///
    /// Defaults to `Proxy`, which is the honest direction to be wrong in: a stream with
    /// no claim (the synthetic writer, an older runtime, a model whose graph names don't
    /// match) reads as a proxy, and only an explicit flag upgrades it.
    pub fn depth_provenance(&self) -> Provenance {
        if self.prov_flags & crate::mind_ring::FLAG_RESID_MEASURED != 0 {
            Provenance::Measured
        } else {
            Provenance::Proxy
        }
    }

    /// As [`Self::depth_provenance`], for the **MLP / FFN** widget (`ffn_out-{N}`).
    /// Tracked separately because a capture can land one channel and miss the other.
    pub fn mlp_provenance(&self) -> Provenance {
        if self.prov_flags & crate::mind_ring::FLAG_MLP_MEASURED != 0 {
            Provenance::Measured
        } else {
            Provenance::Proxy
        }
    }

    /// Decay every live bar/heat cell toward zero (idle settle). Peaks fall too.
    fn idle_decay(&mut self, dt: f32) {
        let k = (1.0 - IDLE_FALL * dt).max(0.0);
        for v in self.depth.iter_mut().chain(self.mlp.iter_mut()).chain(self.heat.iter_mut()) {
            *v *= k;
        }
        for p in self.depth_peaks.iter_mut().chain(self.mlp_peaks.iter_mut()) {
            *p = decay_peak(*p, 0.0, PEAK_FALL, dt);
        }
    }

    // ── Paint (thin egui draws over the computed buffers) ────────────────────

    /// Depth profile RTA — a bar per layer of current activation norm (x = depth,
    /// height = norm) with the Audio-panel peak-hold cap. Watch the bulge climb.
    pub fn paint_depth(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG);
        let inner = rect.shrink(3.0);
        let n = self.depth.len();
        if n == 0 {
            return;
        }
        let gap = if n > 48 { 0.0 } else { 1.0 };
        let bw = ((inner.width() - gap * (n as f32 - 1.0)) / n as f32).max(1.0);
        for i in 0..n {
            let t = self.depth[i];
            let x0 = inner.left() + i as f32 * (bw + gap);
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0, inner.bottom() - inner.height() * t),
                egui::pos2(x0 + bw, inner.bottom()),
            );
            p.rect_filled(bar, egui::CornerRadius::same(0), activity_color(t));
            let py = inner.bottom() - inner.height() * self.depth_peaks[i];
            let cap = egui::Rect::from_min_max(egui::pos2(x0, py - 1.5), egui::pos2(x0 + bw, py));
            p.rect_filled(cap, egui::CornerRadius::same(0), CAP_COL);
        }
    }

    /// Head × layer heat strip — the `head_summ` matrix (y = layer, x = head),
    /// brightness = attention activity. The dashboard's "spectrogram".
    pub fn paint_heat(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG_DEEP);
        let inner = rect.shrink(2.0);
        let (n, h) = (self.n_layers, self.n_heads);
        if n == 0 || h == 0 || self.heat.len() < n * h {
            return;
        }
        let cw = (inner.width() / h as f32).max(1.0);
        let ch = (inner.height() / n as f32).max(1.0);
        for l in 0..n {
            let y = inner.top() + l as f32 * ch;
            for hd in 0..h {
                let t = self.heat[l * h + hd];
                if t <= 0.01 {
                    continue;
                }
                let x = inner.left() + hd as f32 * cw;
                p.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cw.max(1.0), ch.max(1.0))),
                    egui::CornerRadius::same(0),
                    heat_color(t),
                );
            }
        }
    }

    /// MLP rail — a thin per-layer companion strip of `mlp_act` (a horizontal
    /// gradient bar per layer). Reads as the depth profile's quieter twin.
    pub fn paint_mlp(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG);
        let inner = rect.shrink(2.0);
        let n = self.mlp.len();
        if n == 0 {
            return;
        }
        let bw = (inner.width() / n as f32).max(1.0);
        for i in 0..n {
            let t = self.mlp[i];
            let x0 = inner.left() + i as f32 * bw;
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0, inner.bottom() - inner.height() * t),
                egui::pos2(x0 + bw.max(1.0), inner.bottom()),
            );
            p.rect_filled(bar, egui::CornerRadius::same(0), mlp_color(t));
        }
    }

    /// Effort scope — a scrolling waveform of the per-token aggregate activity
    /// (one sample per token), the oscilloscope analog. Oldest left, newest right.
    pub fn paint_effort(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG_DEEP);
        let inner = rect.shrink(3.0);
        // Baseline + a mid gridline.
        let mid = inner.center().y;
        p.hline(inner.x_range(), mid, egui::Stroke::new(1.0, GRID));
        let n = self.effort.len();
        if n < 2 {
            return;
        }
        let dx = inner.width() / (SCOPE_CAP.max(2) - 1) as f32;
        // Right-align the trace so the newest token sits at the right edge.
        let x_off = inner.right() - dx * (n as f32 - 1.0);
        let pt = |i: usize| {
            let t = self.effort[i].clamp(0.0, 1.0);
            egui::pos2(x_off + i as f32 * dx, inner.bottom() - inner.height() * t)
        };
        // Filled area under the trace for a fuller, VU-ish read.
        for i in 1..n {
            let a = pt(i - 1);
            let b = pt(i);
            let t = self.effort[i];
            p.line_segment([a, b], egui::Stroke::new(1.6, activity_color(t)));
        }
        // Playhead dot on the newest sample.
        let head = pt(n - 1);
        p.circle_filled(head, 2.2, CAP_COL);
    }

    /// Heartbeat (#482 Tier 2) — dual scrolling traces of the measured next-token
    /// **confidence** (green, rising = decisive) and **entropy** (amber, rising =
    /// torn), one sample per token. Before any Tier-2 stats arrive it falls back to the
    /// Tier-1 aggregate-**effort** proxy trace, so the widget is never blank.
    pub fn paint_heartbeat(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG_DEEP);
        let inner = rect.shrink(3.0);
        p.hline(inner.x_range(), inner.center().y, egui::Stroke::new(1.0, GRID));
        if self.has_stats() {
            draw_trace(p, inner, &self.conf_hist, CONF_COL, false);
            draw_trace(p, inner, &self.entropy_hist, ENT_COL, true);
            // Tiny legend so the two traces are legible.
            p.text(
                egui::pos2(inner.left() + 2.0, inner.top() + 1.0),
                egui::Align2::LEFT_TOP,
                "entropy",
                egui::FontId::proportional(9.0),
                ENT_COL,
            );
            p.text(
                egui::pos2(inner.left() + 2.0, inner.top() + 12.0),
                egui::Align2::LEFT_TOP,
                "confidence",
                egui::FontId::proportional(9.0),
                CONF_COL,
            );
        } else {
            draw_trace(p, inner, &self.effort, EFFORT_COL, true);
        }
    }

    /// Top-k next-token bars (#482 Tier 2) — one horizontal bar per candidate, width ∝
    /// probability (normalized to the top candidate so the winner fills), labelled with
    /// the decoded token snippet + its absolute probability. The "watch it weigh the /
    /// a / this…" view — the actual softmax, the most legible "what is it doing now".
    pub fn paint_topk(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG);
        let inner = rect.shrink(4.0);
        let k = self.topk_count.min(self.topk_prob.len()).min(self.topk_text.len());
        if k == 0 {
            return;
        }
        let rh = (inner.height() / k as f32).clamp(10.0, 22.0);
        let top = self.topk_prob.first().copied().unwrap_or(0.0).max(1.0e-4);
        for r in 0..k {
            let y = inner.top() + r as f32 * rh;
            let prob = self.topk_prob[r];
            let w = ((prob / top).clamp(0.0, 1.0)) * inner.width();
            let bar = egui::Rect::from_min_max(
                egui::pos2(inner.left(), y + 1.0),
                egui::pos2(inner.left() + w.max(1.0), y + rh - 1.0),
            );
            p.rect_filled(bar, egui::CornerRadius::same(2), activity_color(prob));
            let cy = y + rh * 0.5;
            p.text(
                egui::pos2(inner.left() + 4.0, cy),
                egui::Align2::LEFT_CENTER,
                &self.topk_text[r],
                egui::FontId::monospace((rh - 6.0).clamp(9.0, 13.0)),
                CAP_COL,
            );
            p.text(
                egui::pos2(inner.right() - 2.0, cy),
                egui::Align2::RIGHT_CENTER,
                format!("{:.0}%", prob * 100.0),
                egui::FontId::monospace(9.0),
                egui::Color32::from_rgb(150, 156, 174),
            );
        }
    }

    /// Context / KV fuel gauge (#482 Tier 3) — a horizontal meter of how full the
    /// model's context window is (`ctx_used / ctx_total`), the segmented green→amber→red
    /// gradient the Audio load bar uses, with a numeric readout. Idle holds whatever the
    /// last run left; no stream → an empty "no context" state.
    pub fn paint_context(&self, p: &egui::Painter, rect: egui::Rect) {
        p.rect_filled(rect, egui::CornerRadius::same(3), BG);
        let inner = rect.shrink(3.0);
        if self.ctx_total == 0 {
            p.text(
                inner.center(),
                egui::Align2::CENTER_CENTER,
                "no context",
                egui::FontId::proportional(10.0),
                egui::Color32::from_rgb(110, 116, 130),
            );
            return;
        }
        let frac = (self.ctx_used as f32 / self.ctx_total as f32).clamp(0.0, 1.0);
        let segs = 48;
        for s in 0..segs {
            let f0 = s as f32 / segs as f32;
            if f0 >= frac {
                break;
            }
            let f1 = ((s + 1) as f32 / segs as f32).min(frac);
            let seg = egui::Rect::from_min_max(
                egui::pos2(inner.left() + inner.width() * f0, inner.top()),
                egui::pos2(inner.left() + inner.width() * f1, inner.bottom()),
            );
            p.rect_filled(seg, egui::CornerRadius::same(0), gauge_color(f0));
        }
        p.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            format!("{} / {} tokens · {:.0}%", self.ctx_used, self.ctx_total, frac * 100.0),
            egui::FontId::monospace(10.0),
            CAP_COL,
        );
    }
}

// ── Provenance tags (#482 Tier 3 — the papers' one-rule honesty mark) ─────────

/// How a widget's signal is sourced — a small glyph, not a paragraph. The proxy tags
/// flip to `Measured` the day the `cb_eval` per-layer tap lands; the widget is unchanged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Read straight from the model's actual output (the real softmax / KV counts).
    Measured,
    /// Computed from measured quantities (e.g. tokens/sec from frame timing).
    Derived,
    /// Shaped from a global effort signal, not the true per-layer tensors.
    Proxy,
}

impl Provenance {
    pub fn glyph(self) -> &'static str {
        match self {
            Provenance::Measured => "=",
            Provenance::Derived => "~",
            Provenance::Proxy => "?",
        }
    }
    fn color(self) -> egui::Color32 {
        match self {
            Provenance::Measured => egui::Color32::from_rgb(96, 206, 132),
            Provenance::Derived => egui::Color32::from_rgb(90, 190, 214),
            Provenance::Proxy => egui::Color32::from_rgb(176, 158, 118),
        }
    }
    fn hover(self) -> &'static str {
        match self {
            Provenance::Measured => {
                "measured — read straight from the model's actual next-token softmax / KV counts."
            }
            Provenance::Derived => {
                "derived — computed from measured quantities (e.g. tokens/sec from frame timing)."
            }
            Provenance::Proxy => {
                "proxy — shaped from a global effort signal, not the true per-layer tensors. \
                 Flips to measured the day the cb_eval tap lands; the widget never changes, \
                 only this tag."
            }
        }
    }
}

/// Draw the provenance badge (a coloured glyph, hover-explained) followed by a widget
/// description on one row. The instrument-panel honesty mark, consistent across widgets.
pub fn provenance_row(ui: &mut egui::Ui, prov: Provenance, desc: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(
            egui::RichText::new(prov.glyph())
                .monospace()
                .strong()
                .color(prov.color()),
        )
        .on_hover_text(prov.hover());
        ui.label(egui::RichText::new(desc).small().weak());
    });
}

/// Draw a right-aligned scrolling polyline of a 0..1 scroll buffer into `inner`,
/// newest at the right edge, with an optional playhead dot on the newest sample.
fn draw_trace(
    p: &egui::Painter,
    inner: egui::Rect,
    buf: &[f32],
    color: egui::Color32,
    head_dot: bool,
) {
    let n = buf.len();
    if n < 2 {
        return;
    }
    let dx = inner.width() / (SCOPE_CAP.max(2) - 1) as f32;
    let x_off = inner.right() - dx * (n as f32 - 1.0);
    let pt = |i: usize| {
        let t = buf[i].clamp(0.0, 1.0);
        egui::pos2(x_off + i as f32 * dx, inner.bottom() - inner.height() * t)
    };
    for i in 1..n {
        p.line_segment([pt(i - 1), pt(i)], egui::Stroke::new(1.6, color));
    }
    if head_dot {
        p.circle_filled(pt(n - 1), 2.0, color);
    }
}

/// Make a decoded token snippet safe + legible for a one-line label: control chars
/// become visible glyphs (so a "\n" candidate doesn't wrap the label), and an empty
/// snippet reads as a visible space marker.
fn sanitize_snippet(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push('⏎'),
            '\t' => out.push('⇥'),
            '\r' => {}
            c if c.is_control() => out.push('·'),
            c => out.push(c),
        }
    }
    if out.is_empty() {
        out.push('␣');
    }
    out
}

/// Resize a `Vec<f32>` to `len`, zero-filling growth (shrink drops the tail).
fn resize_to(v: &mut Vec<f32>, len: usize) {
    if v.len() != len {
        v.resize(len, 0.0);
    }
}

// ── Palette (a cool→hot "mind" ramp, distinct from the audio green→red) ──────

const BG: egui::Color32 = egui::Color32::from_rgb(10, 12, 20);
const BG_DEEP: egui::Color32 = egui::Color32::from_rgb(6, 8, 14);
const GRID: egui::Color32 = egui::Color32::from_rgb(34, 40, 56);
const CAP_COL: egui::Color32 = egui::Color32::from_rgb(224, 230, 244);
/// Heartbeat traces (#482 Tier 2): entropy = amber (uncertainty), confidence = green
/// (decisiveness), effort = the Tier-1 cyan proxy fallback.
const ENT_COL: egui::Color32 = egui::Color32::from_rgb(255, 168, 74);
const CONF_COL: egui::Color32 = egui::Color32::from_rgb(96, 206, 132);
const EFFORT_COL: egui::Color32 = egui::Color32::from_rgb(90, 190, 214);

fn lerp3(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    egui::Color32::from_rgb(l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

/// Indigo → cyan → amber → hot-coral, by activity fraction. The main bar ramp.
fn activity_color(t: f32) -> egui::Color32 {
    let a = (58, 74, 168); // indigo
    let b = (58, 196, 210); // cyan
    let c = (255, 181, 71); // amber (the app ACCENT)
    let d = (235, 96, 70); // coral
    if t < 0.4 {
        lerp3(a, b, t / 0.4)
    } else if t < 0.75 {
        lerp3(b, c, (t - 0.4) / 0.35)
    } else {
        lerp3(c, d, (t - 0.75) / 0.25)
    }
}

/// Dimmer indigo→violet ramp for the MLP companion rail.
fn mlp_color(t: f32) -> egui::Color32 {
    lerp3((44, 54, 120), (168, 120, 232), t)
}

/// Fuel-gauge ramp (green with headroom → amber → red as the window fills), by fill
/// fraction — the context gauge reads hot as it approaches full.
fn gauge_color(t: f32) -> egui::Color32 {
    if t < 0.75 {
        lerp3((90, 200, 120), (255, 181, 71), t / 0.75)
    } else {
        lerp3((255, 181, 71), (235, 80, 60), (t - 0.75) / 0.25)
    }
}

/// Black-body-ish heat for the head × layer strip (dark → violet → amber → white).
fn heat_color(t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp3((18, 14, 40), (150, 60, 190), t / 0.5)
    } else if t < 0.85 {
        lerp3((150, 60, 190), (255, 176, 74), (t - 0.5) / 0.35)
    } else {
        lerp3((255, 176, 74), (255, 248, 232), (t - 0.85) / 0.15)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u32, token: u32, n_layers: u32, n_heads: u32, scale: f32) -> MindFrame {
        let mut f = MindFrame::default();
        f.seq = seq;
        f.token_index = token;
        f.n_layers = n_layers;
        f.n_heads = n_heads;
        for l in 0..n_layers as usize {
            f.layer_norm[l] = scale * (l as f32 + 1.0);
            f.mlp_act[l] = scale * 0.5 * (l as f32 + 1.0);
            for h in 0..n_heads as usize {
                f.head_summ[l * MR_MAX_HEADS + h] = scale * ((l + h) as f32 + 1.0);
            }
        }
        f
    }

    #[test]
    fn ema_pulls_toward_sample() {
        assert!((ema(0.0, 1.0, 0.5) - 0.5).abs() < 1e-6);
        assert!((ema(2.0, 2.0, 0.5) - 2.0).abs() < 1e-6);
        // alpha clamps.
        assert!((ema(0.0, 1.0, 5.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decay_peak_jumps_up_and_falls() {
        // Jump up instantly.
        assert!((decay_peak(0.2, 0.9, 0.9, 0.016) - 0.9).abs() < 1e-6);
        // Fall toward cur but never below it.
        let p = decay_peak(0.9, 0.1, 0.9, 0.1); // 0.9 - 0.09 = 0.81
        assert!((p - 0.81).abs() < 1e-5);
        let floored = decay_peak(0.05, 0.1, 0.9, 1.0);
        assert!((floored - 0.1).abs() < 1e-6, "never below cur");
    }

    #[test]
    fn track_gain_rises_instant_decays_relative() {
        let g = track_gain(1.0, 4.0, 0.4, 0.016);
        assert!((g - 4.0).abs() < 1e-6, "rise is instant");
        let g2 = track_gain(4.0, 0.0, 0.5, 1.0); // 4 * (1-0.5) = 2
        assert!((g2 - 2.0).abs() < 1e-4);
        // Floored so it never collapses to zero.
        let g3 = track_gain(GAIN_EPS, 0.0, 0.4, 1.0);
        assert!(g3 >= GAIN_EPS);
    }

    #[test]
    fn frame_activity_is_mean_norm() {
        let f = frame(1, 0, 4, 2, 1.0); // layer_norm = 1,2,3,4 → mean 2.5
        assert!((frame_activity(&f) - 2.5).abs() < 1e-5);
        assert_eq!(frame_activity(&MindFrame::default()), 0.0);
    }

    #[test]
    fn frame_max_covers_norm_and_mlp() {
        let f = frame(1, 0, 3, 2, 2.0); // layer_norm max = 6, mlp max = 3
        assert!((frame_max(&f) - 6.0).abs() < 1e-5);
    }

    #[test]
    fn push_scroll_bounds_and_orders() {
        let mut b = Vec::new();
        for i in 0..(SCOPE_CAP + 10) {
            push_scroll(&mut b, i as f32, SCOPE_CAP);
        }
        assert_eq!(b.len(), SCOPE_CAP, "capped");
        assert_eq!(*b.last().unwrap(), (SCOPE_CAP + 9) as f32, "newest at tail");
        assert_eq!(b[0], 10.0, "oldest dropped");
    }

    #[test]
    fn observe_none_is_flat_idle() {
        let mut v = MindViz::default();
        // A live frame lights it up...
        v.observe(Some(&frame(1, 0, 8, 4, 1.0)), 0.0, 0.016);
        assert!(v.active);
        assert!(v.depth.iter().any(|&x| x > 0.0));
        // ...then no writer decays it to flat idle.
        for i in 0..200 {
            v.observe(None, 1.0 + i as f64 * 0.05, 0.05);
        }
        assert!(!v.active);
        assert!(v.tps < 0.5, "tokens/sec falls to ~0");
        assert!(v.depth.iter().all(|&x| x < 1e-2), "bars settle to zero");
    }

    #[test]
    fn observe_pushes_one_effort_sample_per_new_token() {
        let mut v = MindViz::default();
        // Same seq repainted many times → a single scope sample.
        for i in 0..5 {
            v.observe(Some(&frame(1, 0, 8, 4, 1.0)), i as f64 * 0.016, 0.016);
        }
        assert_eq!(v.effort.len(), 1, "one sample for one token, many repaints");
        // New tokens advance the scope.
        v.observe(Some(&frame(2, 1, 8, 4, 1.0)), 0.1, 0.016);
        v.observe(Some(&frame(3, 2, 8, 4, 1.0)), 0.15, 0.016);
        assert_eq!(v.effort.len(), 3);
        assert_eq!(v.token_index, 2);
        assert!(v.tps > 0.0, "tokens/sec measured from token edges");
    }

    #[test]
    fn observe_normalizes_cross_model_scale() {
        // A huge-magnitude model and a tiny one both fill the widget (auto-gain).
        for scale in [0.01f32, 1000.0] {
            let mut v = MindViz::default();
            v.observe(Some(&frame(1, 0, 6, 3, scale)), 0.0, 0.016);
            let top = v.depth.iter().cloned().fold(0.0f32, f32::max);
            assert!(top > 0.9, "loudest layer near full scale for scale {scale}");
            assert!(v.depth.iter().all(|&x| x <= 1.0), "clamped 0..1");
        }
    }

    fn frame_with_stats(seq: u32, token: u32, entropy: f32, confidence: f32) -> MindFrame {
        let mut f = frame(seq, token, 8, 4, 1.0);
        f.entropy = entropy;
        f.confidence = confidence;
        f.topk_count = 3;
        f.topk_prob = [0.6, 0.25, 0.15, 0.0, 0.0, 0.0, 0.0, 0.0];
        crate::mind_ring::write_snippet(&mut f.topk_text[0], "the");
        crate::mind_ring::write_snippet(&mut f.topk_text[1], " a");
        crate::mind_ring::write_snippet(&mut f.topk_text[2], "\n");
        f
    }

    #[test]
    fn observe_pushes_stats_per_token_and_populates_topk() {
        let mut v = MindViz::default();
        assert!(!v.has_stats(), "no stats before any frame");
        // Many repaints of one token → a single entropy/confidence sample.
        for i in 0..4 {
            v.observe(Some(&frame_with_stats(1, 0, 0.7, 0.4)), i as f64 * 0.016, 0.016);
        }
        assert_eq!(v.entropy_hist.len(), 1);
        assert_eq!(v.conf_hist.len(), 1);
        assert!((v.entropy_hist[0] - 0.7).abs() < 1e-6);
        assert!((v.conf_hist[0] - 0.4).abs() < 1e-6);
        assert!(v.has_stats());
        // Top-k refreshed each frame: 3 bars, snippet sanitized (newline → glyph).
        assert_eq!(v.topk_count, 3);
        assert_eq!(v.topk_text[0], "the");
        assert_eq!(v.topk_text[2], "⏎", "control char sanitized, never a raw newline");
        // A new token advances the traces.
        v.observe(Some(&frame_with_stats(2, 1, 0.2, 0.9)), 0.1, 0.016);
        assert_eq!(v.entropy_hist.len(), 2);
        assert!((v.conf_hist[1] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn observe_tracks_context_gauge() {
        let mut v = MindViz::default();
        let mut f = frame_with_stats(1, 0, 0.5, 0.5);
        f.ctx_used = 137;
        f.ctx_total = 4096;
        v.observe(Some(&f), 0.0, 0.016);
        assert_eq!(v.ctx_used, 137);
        assert_eq!(v.ctx_total, 4096);
        // No stream → the gauge reads empty.
        v.observe(None, 1.0, 0.05);
        assert_eq!(v.ctx_used, 0);
        assert_eq!(v.ctx_total, 0);
    }

    #[test]
    fn provenance_glyphs_are_the_three_marks() {
        assert_eq!(Provenance::Measured.glyph(), "=");
        assert_eq!(Provenance::Derived.glyph(), "~");
        assert_eq!(Provenance::Proxy.glyph(), "?");
    }

    #[test]
    fn observe_clears_topk_when_idle() {
        let mut v = MindViz::default();
        v.observe(Some(&frame_with_stats(1, 0, 0.7, 0.4)), 0.0, 0.016);
        assert_eq!(v.topk_count, 3);
        // No writer → bars clear, but the heartbeat history is retained (frozen trace).
        v.observe(None, 1.0, 0.05);
        assert_eq!(v.topk_count, 0);
        assert!(v.topk_prob.is_empty());
        assert!(v.has_stats(), "entropy/confidence history persists across idle");
    }

    #[test]
    fn observe_resizes_to_frame_dims() {
        let mut v = MindViz::default();
        v.observe(Some(&frame(1, 0, 12, 8, 1.0)), 0.0, 0.016);
        assert_eq!(v.depth.len(), 12);
        assert_eq!(v.heat.len(), 12 * 8);
        // A different model reshapes the buffers.
        v.observe(Some(&frame(2, 1, 4, 2, 1.0)), 0.05, 0.016);
        assert_eq!(v.depth.len(), 4);
        assert_eq!(v.heat.len(), 4 * 2);
    }
}

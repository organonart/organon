//! **Posture — how the console holds itself, on an axis of its own.**
//!
//! [`crate::theme`] answers *what the console is made of*. This answers *how it stands*:
//! tight and square and flush to the left edge like a terminal, or open and inset and
//! quiet like a desktop document. The two are **orthogonal on purpose** — `organon` at
//! desktop posture and a light palette at terminal posture are both real things, and
//! neither is a variant of the other. A palette that also decided the padding would make
//! "the phosphor look, but roomier" unsayable, which is precisely the request this axis
//! exists to answer.
//!
//! ⚠️ **There is now a THIRD axis beside these two, and it was very nearly folded into this
//! one.** [`crate::screen`] — whether the window covers the display — was asked for as *"a new
//! posture, which is full screen"*, and the answer is the same argument this file opens with,
//! applied a second time: a full-screen terminal and a full-screen desktop document are both
//! real, so neither is a variant of the other and neither is a *point* between them. It could
//! not have been one anyway, for the reason the next section gives: there are no slots on this
//! axis to add a third to. That module's header owns the full argument, including why the
//! window's width is deliberately not allowed to feed back into a [`Form`].
//!
//! # Why a scalar rather than two modes
//!
//! **Every form token here is a scalar, and scalars lerp.** The desktop state is not a
//! second renderer and not a second set of draw calls — it is the same draw code reading
//! different numbers. So posture is one `t ∈ [0,1]`, [`Posture`], and the numbers a frame
//! draws with are [`Form::at`] of that `t`. A mode enum would have made every intermediate
//! value unrepresentable and every draw site a branch; a scalar makes Tier C's animation a
//! change to *one field* rather than a rewrite of the drawing.
//!
//! # One owner, no globals — the same rule the theme has
//!
//! `Posture` is held by the console's app state (`Console`, in `native/src/console_main.rs`)
//! beside its `Theme`, and the resolved `Form` reaches a draw site as `&Form`, passed down
//! like any other argument. No `static`, no `thread_local!`, no `OnceCell`. A posture
//! reachable from anywhere stops being state, and a per-tab posture — a terminal tab beside
//! a conversation tab standing differently — becomes a rewrite instead of a second value.
//!
//! # 🚨 Two tokens genuinely do not interpolate, and they are LEFT OUT rather than faked
//!
//! **Font family** (mono ↔ sans) and **label case** (`command:` ↔ `COMMAND:`) are the two
//! form decisions in James's design that are not scalars. There is no half-mono face and no
//! half-capital letter; anything that claimed to lerp them would be a lie written in code
//! that compiles. They are therefore **not fields here at all** — not a threshold, not a
//! snap — and the console's font choices and label spellings are exactly what they were.
//!
//! The alternative considered and rejected was snapping both at, say, `t = 0.5`. It is
//! cheap to write and it is the wrong shape for two reasons: a snap inside a tween is
//! visible as a lurch precisely when the tween is meant to read as one motion (Tier C's
//! problem, created here), and it puts a *threshold constant* in the same struct as a set of
//! genuinely continuous values, so a reader can no longer tell by looking which fields are
//! honest. Leaving them out costs nothing today — nothing draws a sans face or a capitalised
//! label — and leaves the decision where it belongs, with whoever can look at a window.
//!
//! # What posture does NOT govern
//!
//! The **terminal host** ([`crate::term_view`]). A character grid's form is the font's: its
//! cell size, its baseline and its wrap are all consequences of the glyph metrics, and there
//! is no padding, corner or gap in it for a scalar to move. Posture is the conversation
//! front-end's, and the tab strip, the composer plate and the status band are chrome rather
//! than cards — they keep their own constants, and a tier that wants them to breathe should
//! say so out loud rather than inherit it from a token named "card".

use egui::{Color32, CornerRadius, Margin, Stroke};

/// Where the console stands on the terminal ⟷ desktop axis.
///
/// `0.0` is the terminal — what the console has always looked like. `1.0` is the desktop
/// form. Anything between is a real, drawable state: that is the whole point of the type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Posture(f32);

impl Posture {
    /// Flush, tight, square — the look that shipped.
    pub const TERMINAL: Self = Self(0.0);
    /// Inset, open, ruled — James's desktop form.
    pub const DESKTOP: Self = Self(1.0);

    /// Clamped to `[0,1]`, and **NaN resolves to the terminal end** rather than
    /// propagating: a `NaN` here would reach `Margin`'s `i8` and `CornerRadius`'s `u8`
    /// through `as` casts, where it converts to `0` silently, so a single bad float would
    /// square every corner in the window and report nothing. Resolving it at the one place
    /// it can enter is the only spot where it is still a number anyone could notice.
    pub fn new(t: f32) -> Self {
        Self(clamp01(t))
    }

    /// The scalar, for anyone who needs the position rather than the numbers.
    pub fn t(self) -> f32 {
        self.0
    }

    /// The tokens at this posture.
    pub fn form(self) -> Form {
        Form::at(self.0)
    }

    /// A **typed** posture: refused if it is not finite or not in `[0,1]`.
    ///
    /// 🚨 **The asymmetry with [`Posture::new`] is deliberate, and it is
    /// `CameraFraming::in_range`'s exactly.** `new` clamps because it is fed by code — a tween
    /// that overshoots by a rounding error should land at the end, not blow up. A number
    /// somebody *typed* is different: `posture 90` is not an overshoot, it is degrees where
    /// the axis wanted a fraction, and answering `1.0` would let the mistake look like it
    /// worked. The clamp stays as the belt; this is the brace.
    pub fn from_scalar(t: f32) -> Option<Self> {
        (t.is_finite() && (0.0..=1.0).contains(&t)).then_some(Self(t))
    }

    /// The posture a **word or a number** names, or a refusal carrying what would have worked.
    ///
    /// The two words are the ends anybody actually asks for; the bare scalar is the axis
    /// itself, and it is accepted because [`Posture`] *is* a scalar — refusing `0.5` would
    /// mean the CLI could not say a thing the type can represent and `Form::at` can draw.
    ///
    /// ⚠️ **No case folding and no prefixes** — `Theme::resolve`'s rule and
    /// `organon_core::kind::Kind::resolve`'s before it. An approximation here would move a
    /// layout somebody is looking at, to a place they did not name.
    pub fn resolve(word: &str) -> Result<Self, UnknownPosture> {
        match word {
            "terminal" => return Ok(Self::TERMINAL),
            "desktop" => return Ok(Self::DESKTOP),
            _ => {}
        }
        word.parse::<f32>()
            .ok()
            .and_then(Self::from_scalar)
            .ok_or_else(|| UnknownPosture { word: word.to_string() })
    }
}

/// The posture words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`, by the console's command schema and by
/// [`Posture::resolve`]'s refusal — `cli::PORTAL_WORDS`' arrangement, for its reason: a second
/// hand-maintained copy is how a CLI comes to accept a word nothing can act on.
pub const POSTURE_WORDS: &[&str] = &["terminal", "desktop"];

/// A word no posture answers to, carrying the words — and the band — that do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPosture {
    /// Exactly what was asked for, unmodified.
    pub word: String,
}

impl std::fmt::Display for UnknownPosture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is not a posture — known postures: {}, or a number from 0 (terminal) to 1 \
             (desktop)",
            self.word,
            POSTURE_WORDS.join(", ")
        )
    }
}

impl Default for Posture {
    /// The terminal end. A construction site that says nothing gets the look that shipped —
    /// the same rule [`crate::theme::Theme::default`] follows.
    fn default() -> Self {
        Self::TERMINAL
    }
}

/// The form tokens, resolved at one posture. Plain scalars, one per decision, in the units
/// the draw site wants them in (points, alphas, and one multiplier).
///
/// Every field is `f32` even where the draw site needs an integer, because the *token* is
/// continuous and the rounding is the draw site's business — a `u8` corner radius that
/// interpolated in eight steps would stutter where a float does not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Form {
    /// The empty column down **each** side of the conversation, in points. The same number
    /// left and right, so the transcript sits centred in the pane.
    ///
    /// 🚨 **Symmetric, and the left-only version it replaces was a misreading rather than a
    /// design.** The token began as a `gutter` applied on the left alone, from a written
    /// specification that said "add a narrow empty left margin column, about 90px wide" — but
    /// that sentence was written to prompt an *image generator* into restyling a screenshot,
    /// so it was describing a picture, not specifying a layout. Rendered, it read as a window
    /// pushed off its own left edge. What was actually wanted is Claude Desktop's shape:
    /// content centred, ~90 points clear on both sides.
    ///
    /// ⚠️ **A margin, not a measure.** This is an *inset*, so the text column is whatever the
    /// pane has left over — on a very wide window the margins stay 90 and the measure grows
    /// with the window. A maximum content width (Claude Desktop's real rule) was considered
    /// and deliberately not taken; `SHELL_ARCHITECTURE.md` §1.6 records the argument and the
    /// finding that makes it a bigger change than it looks — "uncapped" is not a width, so a
    /// cap cannot be one more scalar lerping between the two ends.
    ///
    /// ⚠️ **The margin, not its contents.** Tier D fills the left one with turn ordinals; this
    /// tier only claims the width, so at desktop posture there is 90 points of nothing on each
    /// side. That is deliberate — the reflow is the part that can be wrong, and it is worth
    /// seeing on its own before anything is drawn into it.
    pub margin: f32,
    /// A card's corner radius: the human bubble, a tool card, an approval, an artifact.
    pub card_radius: f32,
    /// A block nested *inside* a card — today, the plate a surface shows before its picture
    /// arrives. Smaller than [`Form::card_radius`] at both ends, because a corner inside a
    /// corner that matched it would read as a mistake.
    pub nested_radius: f32,
    /// A card's internal padding, horizontal and vertical.
    pub card_pad_x: f32,
    pub card_pad_y: f32,
    /// The human bubble's, which is asymmetric today and is therefore its own pair rather
    /// than a second reading of the one above. Merging them would pin the terminal end to a
    /// value the console does not currently draw.
    pub human_pad_x: f32,
    pub human_pad_y: f32,
    /// Body line height as a **multiple of the font's own row height**, not a point value:
    /// a fixed height would break the moment the text size changed, and `1.0` is exactly
    /// "leave it to the font", which is what the console does today.
    pub line_height: f32,
    /// The vertical space between two elements of the flow, in points.
    pub card_gap: f32,
    /// Extra letter spacing on the flow's small labels, in **em** — a fraction of the
    /// label's own font size, so it survives a text-size change. `0.0` is egui's default.
    pub label_tracking: f32,
    /// How present a card's four-sided border is, `0.0`..`1.0`.
    pub card_border: f32,
    /// …and how present the left rule that replaces it.
    ///
    /// 🚨 **Posture owns this scalar; the PALETTE owns whether the rule is visible at all.**
    /// The four-sided box fades out and the rule fades in over one shared lerp, and every
    /// draw site runs that same lerp with no branch in it — a theme that wants pure surface
    /// separation gives `Theme::card_left_rule` zero alpha and gets nothing; one that wants
    /// a hairline gives it a real colour. The rejected alternative was a
    /// `Box | LeftRule | None` enum per theme, which puts a branch in every card draw and
    /// makes the tween *discontinuous* exactly where the enum flips.
    pub left_rule: f32,
    /// How present the four registration ticks at the corners of the conversation area are.
    pub tick: f32,
    /// A tick's arm length in points. **The same at both ends**: what changes about a
    /// registration mark between postures is whether it is there, not how long it is. It is
    /// a field rather than a `const` so the geometry stays in one struct with the alpha that
    /// decides whether anyone sees it.
    pub tick_len: f32,
}

impl Form {
    /// **The terminal end, and every value in it was read out of the code that shipped**
    /// (`git show main:native/organon-console/src/…`) rather than chosen — see
    /// `form_at_terminal_is_the_form_that_shipped`, which pins each one with its source.
    /// That is the tier's whole safety net: `t = 0.0` must draw today's console exactly.
    ///
    /// 🚨 **One exception, and it is recorded rather than reconciled: `card_radius` is `6`
    /// here and the specification says `0` (square).** The console's cards have carried
    /// `CornerRadius::same(6)` since they were written, so a terminal end of `0` would square
    /// every card in the flow the moment this tier landed — a visible change, at the posture
    /// that is supposed to *be* today's console, and checkable only by somebody looking at a
    /// window. Resolved in favour of "change nothing on screen". Flipping it is this number
    /// and the matching one in the test; see `CONSOLE_ARCHITECTURE.md` §1.6.
    pub const TERMINAL: Self = Self {
        margin: 0.0,
        card_radius: 6.0,
        nested_radius: 4.0,
        card_pad_x: 8.0,
        card_pad_y: 8.0,
        human_pad_x: 10.0,
        human_pad_y: 6.0,
        line_height: 1.0,
        card_gap: 8.0,
        label_tracking: 0.0,
        card_border: 1.0,
        left_rule: 0.0,
        tick: 0.0,
        tick_len: 8.0,
    };

    /// **The desktop end**, from James's specification. Two numbers in it are derived rather
    /// than given, and both are named here so nobody reads them as measured:
    ///
    /// * `card_gap` — the spec says "roomier" and no figure. It takes the padding's number,
    ///   so the space *between* two cards matches the space inside one and the flow has a
    ///   single rhythm rather than two.
    /// * `nested_radius` — the spec gives `8` for a card and `6` for a nested block; the
    ///   terminal end's pair is `6` and `4`, i.e. the same two-point step, which is what
    ///   makes the two ends a translation rather than a re-proportioning.
    pub const DESKTOP: Self = Self {
        margin: 90.0,
        card_radius: 8.0,
        nested_radius: 6.0,
        card_pad_x: 18.0,
        card_pad_y: 18.0,
        human_pad_x: 18.0,
        human_pad_y: 18.0,
        line_height: 1.6,
        card_gap: 18.0,
        label_tracking: 0.13,
        card_border: 0.0,
        left_rule: 1.0,
        tick: 1.0,
        tick_len: 8.0,
    };

    /// The tokens at posture `t`, componentwise linear between the two ends. `t` is clamped
    /// (and NaN resolves to the terminal end) for the reason [`Posture::new`] gives.
    pub fn at(t: f32) -> Self {
        let t = clamp01(t);
        let a = Self::TERMINAL;
        let b = Self::DESKTOP;
        Self {
            margin: lerp(a.margin, b.margin, t),
            card_radius: lerp(a.card_radius, b.card_radius, t),
            nested_radius: lerp(a.nested_radius, b.nested_radius, t),
            card_pad_x: lerp(a.card_pad_x, b.card_pad_x, t),
            card_pad_y: lerp(a.card_pad_y, b.card_pad_y, t),
            human_pad_x: lerp(a.human_pad_x, b.human_pad_x, t),
            human_pad_y: lerp(a.human_pad_y, b.human_pad_y, t),
            line_height: lerp(a.line_height, b.line_height, t),
            card_gap: lerp(a.card_gap, b.card_gap, t),
            label_tracking: lerp(a.label_tracking, b.label_tracking, t),
            card_border: lerp(a.card_border, b.card_border, t),
            left_rule: lerp(a.left_rule, b.left_rule, t),
            tick: lerp(a.tick, b.tick, t),
            tick_len: lerp(a.tick_len, b.tick_len, t),
        }
    }

    // ── The draw sites' spellings ─────────────────────────────────────────
    //
    // Each of these is the one place a token becomes an egui value, so the rounding rule
    // for it is written once instead of at every call. `round()` rather than `as`: a cast
    // truncates, so a lerp passing through 7.99 would spend most of its travel showing 7.

    /// A card's corners.
    pub fn card_corner(&self) -> CornerRadius {
        CornerRadius::same(round_u8(self.card_radius))
    }

    /// A nested block's.
    pub fn nested_corner(&self) -> CornerRadius {
        CornerRadius::same(round_u8(self.nested_radius))
    }

    /// A card's inner margin.
    pub fn card_margin(&self) -> Margin {
        Margin::symmetric(round_i8(self.card_pad_x), round_i8(self.card_pad_y))
    }

    /// The human bubble's.
    pub fn human_margin(&self) -> Margin {
        Margin::symmetric(round_i8(self.human_pad_x), round_i8(self.human_pad_y))
    }

    /// The conversation's margin — **the same on the left and the right**, which is what
    /// centres the content — or `None` when there is not one.
    ///
    /// Horizontal only: nothing about posture wants the scrollback pushed down from the top
    /// of its own scroll area, and the flow's vertical rhythm is [`Form::card_gap`]'s job.
    ///
    /// ⚠️ **`None` is not an optimisation, it is the no-change guarantee.** At terminal
    /// posture the scrollback's walk runs directly in the scroll area's own `Ui`, exactly as
    /// it did before this tier — no wrapping frame, no extra allocation, nothing that could
    /// shift a row by a point. A zero-width margin would *probably* be identical; `None` is
    /// identical by construction.
    pub fn content_margin(&self) -> Option<Margin> {
        (self.margin >= 0.5).then(|| Margin::symmetric(round_i8(self.margin), 0))
    }

    /// An explicit line height in points for body text whose font rows are `row` points
    /// tall, or `None` to leave it to the font.
    ///
    /// Same guarantee as [`Form::content_margin`]: at `1.0` the console passes `None` and
    /// egui lays the text out exactly as it always has, rather than being handed a number
    /// that ought to be the same one.
    pub fn body_line_height(&self, row: f32) -> Option<f32> {
        (self.line_height > 1.0 + f32::EPSILON).then(|| row * self.line_height)
    }

    /// Extra letter spacing in points for a label whose font size is `font_size`.
    pub fn tracking(&self, font_size: f32) -> f32 {
        self.label_tracking * font_size
    }

    /// A card's four-sided border, faded by posture — or `None` once it has gone.
    ///
    /// `None` rather than a transparent stroke because `Frame::stroke` still reserves the
    /// stroke's width in its layout: a zero-alpha 1-point border would leave a 1-point ring
    /// of invisible inset around every card and the desktop form would be a point narrower
    /// than it asked to be.
    pub fn card_stroke(&self, accent: Color32) -> Option<Stroke> {
        fade(accent, self.card_border).map(|c| Stroke::new(1.0f32, c))
    }

    /// The left rule's colour at this posture, or `None` when nothing should be drawn.
    ///
    /// Two independent ways to get `None`, and both matter: posture at the terminal end, and
    /// a palette whose `card_left_rule` is transparent. Neither is a fallback for the other —
    /// the first is "not standing that way", the second is "this palette separates surfaces
    /// without a hairline".
    pub fn left_rule_color(&self, rule: Color32) -> Option<Color32> {
        fade(rule, self.left_rule)
    }

    /// A registration tick's colour at this posture, or `None`.
    pub fn tick_color(&self, mark: Color32) -> Option<Color32> {
        fade(mark, self.tick)
    }
}

impl Default for Form {
    fn default() -> Self {
        Self::TERMINAL
    }
}

/// Multiply a colour's presence by `alpha`, or answer `None` if the result would be
/// invisible.
///
/// `alpha >= 1.0` returns the colour **untouched** rather than round-tripping it through a
/// multiply: `gamma_multiply` works in 8-bit channels, so an identity that went through it
/// could still land a byte off, and this is the path every card takes at terminal posture.
pub fn fade(color: Color32, alpha: f32) -> Option<Color32> {
    if !(alpha > 0.0) {
        // Also catches NaN, which would otherwise multiply every channel to zero *and*
        // report itself as visible.
        return None;
    }
    if color.a() == 0 {
        // The palette's half of the answer, and it is checked *before* the full-strength
        // shortcut below: a colour a theme chose to make invisible must stay invisible at
        // `t = 1.0` too, where no multiply would ever run.
        return None;
    }
    if alpha >= 1.0 {
        return Some(color);
    }
    let faded = color.gamma_multiply(alpha);
    (faded.a() > 0).then_some(faded)
}

fn clamp01(t: f32) -> f32 {
    if t.is_nan() {
        0.0
    } else {
        t.clamp(0.0, 1.0)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn round_u8(v: f32) -> u8 {
    v.round().clamp(0.0, u8::MAX as f32) as u8
}

fn round_i8(v: f32) -> i8 {
    v.round().clamp(i8::MIN as f32, i8::MAX as f32) as i8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The whole safety net for the tier.** Every value below was read out of the code as
    /// it stood on `main` before a line of it moved, so this is pinned against what the
    /// console *draws* rather than against what was typed into [`Form::TERMINAL`]. Nothing
    /// else in the suite can tell whether a padding drifted by a point during the wiring,
    /// because a wrong number compiles and draws.
    #[test]
    fn form_at_terminal_is_the_form_that_shipped() {
        let f = Form::at(0.0);

        // There is no margin today: the scrollback's content runs from the scroll area's own
        // left edge to its own right edge.
        assert_eq!(f.margin, 0.0, "the symmetric content margin");
        // `CornerRadius::same(6)` — conversation_view.rs, at all five card frames: the human
        // bubble, `tool_card`, `approval_card`, `panel_element`, `surface_element`.
        //
        // 🚨 The specification says `0` (square) at this end. It is `6` because the console
        // draws `6`, and the tier's bar is that `t = 0.0` changes nothing on screen — see
        // `Form::TERMINAL`'s doc for the argument and for what flipping it costs.
        assert_eq!(f.card_radius, 6.0, "a card's corner");
        // `CornerRadius::same(4)` — the plate `surface_element` draws before its picture
        // arrives, the one block nested inside a card today.
        assert_eq!(f.nested_radius, 4.0, "a nested block's corner");
        // `Margin::same(8)` on `tool_card` and `approval_card`; `Margin::same(PAD as i8)` on
        // the two artifact frames, where `block_panel::PAD` is `8.0`.
        assert_eq!(f.card_pad_x, 8.0, "a card's horizontal padding");
        assert_eq!(f.card_pad_y, 8.0, "a card's vertical padding");
        // `Margin::symmetric(10, 6)` — the human bubble, the one asymmetric card.
        assert_eq!(f.human_pad_x, 10.0, "the human bubble's horizontal padding");
        assert_eq!(f.human_pad_y, 6.0, "the human bubble's vertical padding");
        // No `line_height` call exists today, i.e. the font's own row height, i.e. ×1.
        assert_eq!(f.line_height, 1.0, "body line height, as a multiple");
        assert!(f.body_line_height(17.0).is_none(), "and therefore no explicit height at all");
        // `ui.add_space(8.0)` after each element in `scrollback`'s walk.
        assert_eq!(f.card_gap, 8.0, "the space between two elements");
        // egui's default `extra_letter_spacing`, which no call site overrides.
        assert_eq!(f.label_tracking, 0.0, "label tracking");
        // `Stroke::new(1.0f32, accent)` — drawn at the accent's own full alpha.
        assert_eq!(f.card_border, 1.0, "the four-sided border");
        // Neither of these exists today at all.
        assert_eq!(f.left_rule, 0.0, "the left rule");
        assert_eq!(f.tick, 0.0, "the registration ticks");
    }

    /// The other end, against James's specification.
    #[test]
    fn form_at_desktop_is_the_specification() {
        let f = Form::at(1.0);
        assert_eq!(f.margin, 90.0, "90 points clear on each side");
        assert_eq!(f.card_radius, 8.0, "8px corners");
        assert_eq!(f.nested_radius, 6.0, "6px on a nested block");
        assert_eq!(f.card_pad_x, 18.0, "~18px of internal padding");
        assert_eq!(f.card_pad_y, 18.0);
        assert_eq!(f.human_pad_x, 18.0, "the human bubble joins them");
        assert_eq!(f.human_pad_y, 18.0);
        assert_eq!(f.line_height, 1.6, "1.6 body line height");
        assert_eq!(f.body_line_height(20.0), Some(32.0), "…which is a real number of points");
        assert_eq!(f.card_gap, 18.0, "roomier, derived from the padding — see DESKTOP");
        assert_eq!(f.label_tracking, 0.13, "0.13em on labels");
        assert!((f.tracking(10.0) - 1.3).abs() < 1e-6, "em, not points: {}", f.tracking(10.0));
        assert_eq!(f.card_border, 0.0, "the four-sided border is gone");
        assert_eq!(f.left_rule, 1.0, "and the left rule has taken over");
        assert_eq!(f.tick, 1.0, "registration ticks, four corners");
        assert_eq!(f.tick_len, 8.0, "8px arms");
    }

    /// 🚨 **The midpoint, so a broken lerp cannot pass.** Pinning only the two ends would be
    /// satisfied by a `Form::at` that ignored `t` and picked the nearer end — which is a
    /// mode enum wearing a scalar's clothes, and the exact mistake this type exists to make
    /// impossible. Every field is checked, because a lerp that dropped one would still put
    /// both ends where they belong.
    #[test]
    fn form_at_the_midpoint_is_halfway_between_the_two_ends() {
        let f = Form::at(0.5);
        assert_eq!(f.margin, 45.0);
        assert_eq!(f.card_radius, 7.0);
        assert_eq!(f.nested_radius, 5.0);
        assert_eq!(f.card_pad_x, 13.0);
        assert_eq!(f.card_pad_y, 13.0);
        assert_eq!(f.human_pad_x, 14.0);
        assert_eq!(f.human_pad_y, 12.0);
        assert!((f.line_height - 1.3).abs() < 1e-6, "{}", f.line_height);
        assert_eq!(f.card_gap, 13.0);
        assert!((f.label_tracking - 0.065).abs() < 1e-6, "{}", f.label_tracking);
        assert_eq!(f.card_border, 0.5);
        assert_eq!(f.left_rule, 0.5);
        assert_eq!(f.tick, 0.5);
        assert_eq!(f.tick_len, 8.0, "the one token that is the same at both ends");
    }

    /// A quarter of the way, on the two tokens whose midpoints happen to be integers anyway —
    /// so the halfway test cannot be passing by arithmetic coincidence.
    #[test]
    fn a_quarter_of_the_way_is_a_quarter_of_the_way() {
        let f = Form::at(0.25);
        assert_eq!(f.margin, 22.5);
        assert_eq!(f.card_pad_x, 10.5);
        assert_eq!(f.human_pad_y, 9.0);
    }

    /// Out of range, and NaN, resolve rather than propagate — for the reason
    /// [`Posture::new`] states: these numbers reach `u8` and `i8` through `as` casts.
    #[test]
    fn posture_clamps_and_nan_is_the_terminal_end() {
        assert_eq!(Posture::new(-3.0).t(), 0.0);
        assert_eq!(Posture::new(4.0).t(), 1.0);
        assert_eq!(Posture::new(f32::NAN).t(), 0.0);
        assert_eq!(Form::at(-3.0), Form::TERMINAL);
        assert_eq!(Form::at(4.0), Form::DESKTOP);
        assert_eq!(Form::at(f32::NAN), Form::TERMINAL);
    }

    /// The two ends are reachable by name and by number, and they agree.
    #[test]
    fn the_named_ends_are_the_numbered_ends() {
        assert_eq!(Posture::TERMINAL.form(), Form::TERMINAL);
        assert_eq!(Posture::DESKTOP.form(), Form::DESKTOP);
        assert_eq!(Posture::default(), Posture::TERMINAL);
        assert_eq!(Form::default(), Form::TERMINAL);
    }

    /// The egui spellings round rather than truncate, so a corner spends half its travel on
    /// each side of a step instead of nearly all of it on the low one.
    #[test]
    fn the_draw_spellings_round() {
        // 6 → 8 over the whole range, so t=0.3 is 6.6 and must read 7, not 6.
        assert_eq!(Form::at(0.3).card_corner(), CornerRadius::same(7));
        assert_eq!(Form::at(0.0).card_corner(), CornerRadius::same(6));
        assert_eq!(Form::at(1.0).card_corner(), CornerRadius::same(8));
        assert_eq!(Form::at(1.0).nested_corner(), CornerRadius::same(6));
        assert_eq!(Form::at(0.0).card_margin(), Margin::symmetric(8, 8));
        assert_eq!(Form::at(1.0).card_margin(), Margin::symmetric(18, 18));
        assert_eq!(Form::at(0.0).human_margin(), Margin::symmetric(10, 6));
        assert_eq!(Form::at(1.0).human_margin(), Margin::symmetric(18, 18));
    }

    /// 🚨 **The two `None`s that are the no-change guarantee**, not optimisations: at the
    /// terminal end the scrollback wraps nothing and the text is laid out by the font, so
    /// there is no wrapper frame and no explicit height to be a point out.
    #[test]
    fn nothing_is_wrapped_or_overridden_at_the_terminal_end() {
        let t = Form::at(0.0);
        assert!(t.content_margin().is_none(), "no wrapping frame around the walk");
        assert!(t.body_line_height(17.0).is_none(), "no explicit line height");
        assert!(t.left_rule_color(Color32::GREEN).is_none(), "no left rule");
        assert!(t.tick_color(Color32::GREEN).is_none(), "no registration ticks");

        let d = Form::at(1.0);
        assert_eq!(d.content_margin(), Some(Margin { left: 90, right: 90, top: 0, bottom: 0 }));
        assert_eq!(d.left_rule_color(Color32::GREEN), Some(Color32::GREEN));
        assert_eq!(d.tick_color(Color32::GREEN), Some(Color32::GREEN));
    }

    /// 🚨 **The bug this token was renamed for: the margin is the SAME on both sides.** The
    /// first cut applied it on the left alone, which read on screen as a window shoved off its
    /// own left edge rather than as a centred document — and every test then passing was
    /// perfectly happy, because they all checked the *scalar* and none of them checked the
    /// `Margin` it became. So this walks the axis and asserts the shape at each step, not just
    /// the two ends: `left == right`, no vertical inset, and the content therefore centred.
    #[test]
    fn the_content_margin_is_symmetric_at_every_posture() {
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let f = Form::at(t);
            match f.content_margin() {
                // Only the bottom of the range: 90 · t clears the half-point threshold from
                // t ≈ 0.0056 upward, so every step above zero has a margin.
                None => assert_eq!(step, 0, "a margin went missing at t={t}"),
                Some(m) => {
                    assert_eq!(m.left, m.right, "asymmetric at t={t}: {m:?}");
                    assert_eq!((m.top, m.bottom), (0, 0), "vertical inset at t={t}: {m:?}");
                    assert_eq!(m.left as f32, f.margin.round(), "not the token at t={t}");
                }
            }
        }
        // The three postures the tests above pin as scalars, spelled as the egui value the
        // draw site actually receives — ends and midpoint, so a `content_margin` that ignored
        // `t` cannot pass either.
        assert_eq!(Form::at(0.0).content_margin(), None);
        assert_eq!(Form::at(0.5).content_margin(), Some(Margin::symmetric(45, 0)));
        assert_eq!(Form::at(1.0).content_margin(), Some(Margin::symmetric(90, 0)));
    }

    /// The border and the rule are **one shared lerp** — the sum of their presences is 1 at
    /// every posture, which is what makes the exchange continuous. This is the property the
    /// rejected `Box | LeftRule | None` enum could not have had.
    #[test]
    fn the_border_hands_over_to_the_rule_without_a_gap() {
        for step in 0..=10 {
            let f = Form::at(step as f32 / 10.0);
            assert!(
                (f.card_border + f.left_rule - 1.0).abs() < 1e-6,
                "at t={}: border {} + rule {}",
                step,
                f.card_border,
                f.left_rule
            );
        }
    }

    /// ⚠️ **A palette can still refuse the rule, and posture cannot override it.** That is
    /// the half of the decision the theme owns: a fully transparent `card_left_rule` yields
    /// nothing at *desktop* posture, with no branch anywhere in the draw path.
    #[test]
    fn a_transparent_palette_rule_stays_invisible_at_every_posture() {
        for step in 0..=10 {
            let f = Form::at(step as f32 / 10.0);
            assert!(f.left_rule_color(Color32::TRANSPARENT).is_none(), "at t={step}");
        }
    }

    /// `fade` at full strength returns the colour it was given, byte for byte — the path
    /// every card's border takes at terminal posture.
    #[test]
    fn a_full_strength_fade_is_the_identity() {
        let c = Color32::from_rgb(0xe6, 0xc0, 0x4c);
        assert_eq!(fade(c, 1.0), Some(c));
        assert_eq!(fade(c, 2.0), Some(c));
        assert_eq!(fade(c, 0.0), None);
        assert_eq!(fade(c, -1.0), None);
        assert_eq!(fade(c, f32::NAN), None);
        // And in between it really does dim.
        let half = fade(c, 0.5).expect("half of an opaque colour is still visible");
        assert_ne!(half, c);
    }

    /// The border stroke disappears entirely rather than becoming transparent, because
    /// `Frame` reserves a stroke's width whether or not it can be seen.
    #[test]
    fn the_card_border_stops_reserving_width_once_it_is_gone() {
        let accent = Color32::from_rgb(0xe6, 0xc0, 0x4c);
        assert_eq!(Form::at(0.0).card_stroke(accent), Some(Stroke::new(1.0f32, accent)));
        assert!(Form::at(1.0).card_stroke(accent).is_none());
    }
}

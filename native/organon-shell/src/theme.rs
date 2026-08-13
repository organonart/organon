//! Every colour the console paints, in one owned value.
//!
//! Before this module the console's palette was ~50 `const Color32` declarations spread
//! across six files, plus a handful of literals written inline at the draw site. Each one
//! already carried a **semantic** name — `RUNNING`, `COMPOSER_EDGE_DEAD`, `CONTEXT_ARC` —
//! so the hard half of theming was done: the code already says what a colour *means*
//! rather than what pigment it is. What it could not do is hold a second answer. A `const`
//! is a fact about the program, not about the session, so "the same console in a lighter
//! palette" had nowhere to live.
//!
//! So this is a plain struct of fields, and deliberately nothing cleverer. Not a trait —
//! there is one implementation and a trait would buy an indirection to hide it behind. Not
//! a `HashMap<&str, Color32>` — a name that can be spelled wrong at the draw site turns a
//! compile error into a missing colour. Not a registry of pigments a role points into —
//! that is the layer that lets two roles get accidentally welded together, which is
//! exactly what a second palette needs to be able to prise apart.
//!
//! # One owner, no globals
//!
//! `Theme` is held by the console's app state (`Shell`, in `native/src/shell_main.rs`) and
//! reaches a draw site as `&Theme`, passed down like any other argument. There is no
//! `static`, no `thread_local!` and no `OnceCell`, and that is the point rather than an
//! aesthetic: the moment a palette is reachable from anywhere it stops being *state* and
//! a second one — a per-tab theme, a preview while a palette is being chosen — becomes a
//! rewrite instead of a second value.
//!
//! # Same value, different role: kept apart on purpose
//!
//! Several fields here hold identical bytes today. `human_text`, `tab_active`,
//! `tab_menu_installed` and `term_fg` are all `#c8e6c8`; `context_arc_high` is
//! `mode_alert`'s amber to the byte, and said so in a comment before it was a field;
//! `timeline_status_denied` equals `timeline_status_failed`. None of them are merged.
//! Deduplicating by value is precisely what makes a palette unable to diverge later — a
//! light theme almost certainly wants the terminal's foreground and a human's typed line
//! to part company, and it cannot do that if one field is serving both.
//!
//! # What a second palette would have to override
//!
//! Every field below, and only these. The console's remaining colour comes from three
//! places this tier does not touch: egui's own `Visuals` (widget chrome, `weak`/`strong`
//! text, `extreme_bg_color`), the xterm 256-colour cube and greyscale ramp — which are a
//! **standard**, not a taste, and are computed rather than stored — and the truecolor and
//! OSC-override values a running program sends, which belong to that program.

use egui::Color32;

/// The console's palette. One value, one owner, borrowed at every draw site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    // ── The transcript ────────────────────────────────────────────────────
    /// A human's own typed line, inside its bubble.
    pub human_text: Color32,
    /// That bubble's plate — the one framed element in the flow, because the human turn
    /// is the only thing in it the human wrote.
    pub human_fill: Color32,
    /// The agent's prose. Unframed: it is the page, not a card on it.
    pub prose: Color32,
    /// Labels, captions, counts — everything present but not being read.
    pub dim: Color32,

    // ── Cards ─────────────────────────────────────────────────────────────
    /// A tool call in flight.
    pub running: Color32,
    /// A question waiting on a human. Deliberately not [`Theme::running`]'s amber: "busy"
    /// and "blocked on you" must not read alike at a glance.
    pub asking: Color32,
    /// A call that finished cleanly.
    pub ok: Color32,
    /// A call that failed, and a denial.
    pub bad: Color32,
    /// The plate a rendered surface shows before the console has drawn into it.
    pub surface_empty: Color32,
    /// **The rule down a card's left edge, which replaces its four-sided border as posture
    /// opens** ([`crate::posture`]).
    ///
    /// 🚨 **The palette decides whether a card is edged at all; posture only decides how
    /// present that edge is.** The box fades out and this rule fades in over one shared
    /// lerp, with no per-theme branch at any draw site — so a palette that separates
    /// surfaces by fill alone gives this a **fully transparent** colour and gets no rule,
    /// while a palette that wants a hairline gives it a real one. `organon` takes the first
    /// answer, which is what keeps this tier invisible: its cards are four-sided boxes and
    /// there was no rule to preserve.
    ///
    /// The rejected alternative was a `Box | LeftRule | None` enum per theme. It puts a
    /// branch in every card draw, and it makes the tween *discontinuous* precisely where the
    /// enum flips — an alpha cannot do either.
    pub card_left_rule: Color32,

    // ── The status strip ──────────────────────────────────────────────────
    /// The band along the bottom, and its hairline.
    pub strip_fill: Color32,
    pub strip_edge: Color32,
    /// The model plate inside it.
    pub model_fill: Color32,
    pub model_edge: Color32,
    /// The model's own name — brighter than [`Theme::prose`], because it is an *identity*
    /// rather than a reading.
    pub model_text: Color32,
    /// The bracketed variant beside it (`1m` → `1M`): present, secondary, never mistaken
    /// for the name.
    pub model_badge: Color32,
    /// The permission plate's two voices.
    ///
    /// ⚠️ **Neither is [`Theme::bad`], and that is the decision.** The non-default marker is
    /// on the band for *hours*, not for a moment, and this is a band a working hand looks at
    /// constantly — a red klaxon that never goes away is a red klaxon the eye learns to skip,
    /// which would leave the console back where it started. Amber is legible against the dim
    /// half without competing with an actual failure.
    pub mode_alert: Color32,
    pub mode_note: Color32,
    /// The context ring's unfilled circumference — present enough to say "this is a dial and
    /// it is not full", dim enough not to compete with the band's actual readings.
    pub context_track: Color32,
    /// …and the same circumference **before anything has been measured**.
    ///
    /// 🚨 **The one visual difference between "no reading yet" and "a reading of 0 %".** Both
    /// draw a bare circle — a zero fill has no arc either — so if the two circles looked
    /// alike the ring would be making the exact false claim the draw-nothing rule existed to
    /// prevent. Sat roughly midway between [`Theme::strip_fill`] and [`Theme::context_track`]:
    /// legible as a shape at arm's length, so it still reads as the container an answer will
    /// appear in, and unmistakably fainter beside a track that is holding a real one. The
    /// hover carries the same distinction in words, because a colour difference alone is not
    /// an answer to "which is this?".
    ///
    /// ⚠️ A second palette has to keep that *ordering*, not merely the two values — the
    /// contract is a test (`an_unmeasured_ring_is_distinguishable_from_a_measured_nought`),
    /// and it asks that the empty track be the fainter of the two and still visible against
    /// the band.
    pub context_track_empty: Color32,
    /// The filled arc below the high-water mark.
    ///
    /// Blue rather than a green off this band's own palette, and that is the point: every
    /// other colour here is a *standing* — [`Theme::running`] busy, [`Theme::asking`]
    /// blocked, [`Theme::bad`] gone — and the ring is not a standing. It is a resource gauge,
    /// true continuously, and giving it a hue no reading uses is what keeps a half-full
    /// context from looking like a state the agent is in.
    pub context_arc: Color32,
    /// …and above it.
    pub context_arc_high: Color32,

    // ── The composer ──────────────────────────────────────────────────────
    /// The composer's plate, and its edge at rest, focused, and dead.
    pub composer_fill: Color32,
    pub composer_edge: Color32,
    pub composer_edge_focus: Color32,
    pub composer_edge_dead: Color32,

    // ── The terminal ──────────────────────────────────────────────────────
    /// The default screen: near-black with a whisper of green, phosphor foreground.
    pub term_bg: Color32,
    pub term_fg: Color32,
    /// The legibility scrim's tint. **Its alpha is not stored here** — that is
    /// `ORGANON_SHELL_SCRIM`'s, floored by [`crate::term_view::scrim_alpha`], and a theme
    /// that could set it would be able to trade the glyphs away. Only the three colour
    /// channels are the palette's.
    pub term_scrim_tint: Color32,
    /// The `[process exited …]` notice under a dead tab.
    pub term_exited_notice: Color32,
    /// The 16 ANSI colours, phosphor-leaning but conventional enough that TUI colour
    /// schemes read as intended. On the theme because a light palette beside a black
    /// terminal is not a theme — it is two products in one window.
    pub ansi16: [Color32; 16],

    // ── Patch panels ──────────────────────────────────────────────────────
    /// A panel's surface: dark enough to read widgets against, translucent enough that the
    /// scene behind the glass still shows. **Premultiplied** — the alpha is the look.
    pub panel_fill: Color32,
    /// Its phosphor hairline, worn by the portal's frame as well.
    pub panel_edge: Color32,
    /// The title line.
    pub panel_title: Color32,
    /// A panel's body text. Its own field rather than [`Theme::term_fg`] reused: a panel is
    /// egui chrome floating over the grid, not a cell in it, and a palette that lightens
    /// the terminal has no reason to be forced to lighten this with it.
    pub panel_text: Color32,

    // ── The timeline (the v2 workspace cards) ─────────────────────────────
    /// The scripted-demo banner's plate and its mark — the honesty rule made visible: a
    /// replay must never pass as a live agent.
    pub timeline_scripted_fill: Color32,
    pub timeline_scripted_mark: Color32,
    /// One colour per [`crate::session::RunStatus`]. Six fields for six statuses even where
    /// two agree today, because "denied" and "failed" are different answers.
    pub timeline_status_pending: Color32,
    pub timeline_status_running: Color32,
    pub timeline_status_ok: Color32,
    pub timeline_status_failed: Color32,
    pub timeline_status_denied: Color32,
    pub timeline_status_cancelled: Color32,
    /// The accent on the one card that asks the human for something.
    pub timeline_approval_accent: Color32,
    /// Message bubbles, sided by issuer.
    pub timeline_bubble_user: Color32,
    pub timeline_bubble_other: Color32,

    // ── The tab strip ─────────────────────────────────────────────────────
    /// The strip itself — the one permitted chrome.
    pub tab_strip_fill: Color32,
    /// A tab's title, active and not.
    pub tab_active: Color32,
    pub tab_inactive: Color32,
    /// The **+** button.
    pub tab_plus: Color32,
    /// The harness menu's plate, and its two entry states: installed and selectable,
    /// missing and greyed.
    pub tab_menu_fill: Color32,
    pub tab_menu_installed: Color32,
    pub tab_menu_missing: Color32,
}

impl Theme {
    /// The console's own look — phosphor green on near-black — and the only palette this
    /// tier ships. Every value is the one the corresponding `const` held before the
    /// extraction; `theme_organon_is_the_look_that_shipped` pins each of them.
    pub const fn organon() -> Self {
        Self {
            human_text: Color32::from_rgb(0xc8, 0xe6, 0xc8),
            human_fill: Color32::from_rgb(0x11, 0x18, 0x11),
            prose: Color32::from_rgb(0xd2, 0xd8, 0xd2),
            dim: Color32::from_rgb(0x70, 0x7c, 0x70),

            running: Color32::from_rgb(0xe6, 0xc0, 0x4c),
            asking: Color32::from_rgb(0x7f, 0xb8, 0xe6),
            ok: Color32::from_rgb(0x6f, 0xc2, 0x76),
            bad: Color32::from_rgb(0xe0, 0x6c, 0x5f),
            surface_empty: Color32::from_rgb(0x0a, 0x0e, 0x0a),
            // Fully transparent, and that is this palette's *answer* rather than a
            // placeholder: the console's cards are four-sided boxes, so there is no left
            // rule to preserve and a visible one would be a change nobody asked for. A
            // palette that wants the desktop form to be ruled says so here.
            card_left_rule: Color32::TRANSPARENT,

            strip_fill: Color32::from_rgb(0x0b, 0x11, 0x0b),
            strip_edge: Color32::from_rgb(0x22, 0x2c, 0x22),
            model_fill: Color32::from_rgb(0x15, 0x1e, 0x15),
            model_edge: Color32::from_rgb(0x3a, 0x50, 0x3a),
            model_text: Color32::from_rgb(0xc6, 0xdf, 0xc6),
            model_badge: Color32::from_rgb(0x8a, 0xb0, 0x8a),
            mode_alert: Color32::from_rgb(0xd8, 0x9a, 0x5c),
            mode_note: Color32::from_rgb(0x8a, 0xa6, 0xc2),
            context_track: Color32::from_rgb(0x2c, 0x38, 0x2c),
            context_track_empty: Color32::from_rgb(0x1a, 0x22, 0x1a),
            context_arc: Color32::from_rgb(0x5f, 0x93, 0xcc),
            // `MODE_ALERT`'s exact amber, written out rather than aliased: it already means
            // "worth acting on, not a failure" on this band, which is precisely the
            // reading — but a palette that re-chose one has no reason to be forced into
            // the other.
            context_arc_high: Color32::from_rgb(0xd8, 0x9a, 0x5c),

            composer_fill: Color32::from_rgb(0x0e, 0x14, 0x0e),
            composer_edge: Color32::from_rgb(0x2b, 0x38, 0x2b),
            composer_edge_focus: Color32::from_rgb(0x4c, 0x7a, 0x52),
            composer_edge_dead: Color32::from_rgb(0x3a, 0x2c, 0x2c),

            term_bg: Color32::from_rgb(0x0a, 0x0d, 0x0a),
            term_fg: Color32::from_rgb(0xc8, 0xe6, 0xc8),
            term_scrim_tint: Color32::from_rgb(0x0a, 0x0d, 0x0a),
            term_exited_notice: Color32::from_rgb(0x80, 0x8a, 0x80),
            ansi16: [
                Color32::from_rgb(0x10, 0x14, 0x10), // black
                Color32::from_rgb(0xcc, 0x52, 0x4b), // red
                Color32::from_rgb(0x5c, 0xb8, 0x5c), // green
                Color32::from_rgb(0xc2, 0xb0, 0x4c), // yellow
                Color32::from_rgb(0x56, 0x92, 0xd8), // blue
                Color32::from_rgb(0xb0, 0x6c, 0xc0), // magenta
                Color32::from_rgb(0x4c, 0xb8, 0xb0), // cyan
                Color32::from_rgb(0xc8, 0xd2, 0xc8), // white
                Color32::from_rgb(0x50, 0x5a, 0x50), // bright black
                Color32::from_rgb(0xe8, 0x6a, 0x62), // bright red
                Color32::from_rgb(0x74, 0xd8, 0x74), // bright green
                Color32::from_rgb(0xdc, 0xcc, 0x66), // bright yellow
                Color32::from_rgb(0x74, 0xac, 0xec), // bright blue
                Color32::from_rgb(0xcc, 0x88, 0xdc), // bright magenta
                Color32::from_rgb(0x68, 0xd4, 0xcc), // bright cyan
                Color32::from_rgb(0xee, 0xf4, 0xee), // bright white
            ],

            panel_fill: Color32::from_rgba_premultiplied(0x0b, 0x12, 0x0e, 0xe6),
            panel_edge: Color32::from_rgb(0x3e, 0x7a, 0x52),
            panel_title: Color32::from_rgb(0x8f, 0xe0, 0xa8),
            panel_text: Color32::from_rgb(0xc8, 0xe6, 0xc8),

            timeline_scripted_fill: Color32::from_rgb(0x33, 0x2b, 0x12),
            timeline_scripted_mark: Color32::from_rgb(0xe6, 0xc0, 0x4c),
            timeline_status_pending: Color32::from_rgb(0xb0, 0xb0, 0xb0),
            timeline_status_running: Color32::from_rgb(0xe6, 0xc0, 0x4c),
            timeline_status_ok: Color32::from_rgb(0x6f, 0xc2, 0x76),
            timeline_status_failed: Color32::from_rgb(0xe0, 0x6c, 0x5f),
            timeline_status_denied: Color32::from_rgb(0xe0, 0x6c, 0x5f),
            timeline_status_cancelled: Color32::from_rgb(0xb0, 0xb0, 0xb0),
            timeline_approval_accent: Color32::from_rgb(0xe6, 0xc0, 0x4c),
            timeline_bubble_user: Color32::from_rgb(0x24, 0x33, 0x42),
            timeline_bubble_other: Color32::from_rgb(0x26, 0x26, 0x2b),

            tab_strip_fill: Color32::from_rgb(0x07, 0x09, 0x07),
            tab_active: Color32::from_rgb(0xc8, 0xe6, 0xc8),
            tab_inactive: Color32::from_rgb(0x60, 0x6c, 0x60),
            tab_plus: Color32::from_rgb(0x8a, 0x96, 0x8a),
            tab_menu_fill: Color32::from_rgb(0x10, 0x14, 0x10),
            tab_menu_installed: Color32::from_rgb(0xc8, 0xe6, 0xc8),
            tab_menu_missing: Color32::from_rgb(0x50, 0x5a, 0x50),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::organon()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The whole safety net for the extraction.** Every value below was read out of
    /// `main` — `git show main:native/organon-shell/src/<file>` — *before* a line of it was
    /// moved, so this test is pinned against the look that shipped rather than against
    /// whatever was typed into `organon()`. The tier's bar is that the console looks
    /// identical; nothing else in the suite can tell whether a shade drifted by one byte
    /// during the move, because a wrong colour compiles and draws.
    #[test]
    fn theme_organon_is_the_look_that_shipped() {
        let t = Theme::organon();

        // conversation_view.rs — the transcript
        assert_eq!(t.human_text, Color32::from_rgb(0xc8, 0xe6, 0xc8), "HUMAN");
        assert_eq!(t.human_fill, Color32::from_rgb(0x11, 0x18, 0x11), "the human bubble's fill");
        assert_eq!(t.prose, Color32::from_rgb(0xd2, 0xd8, 0xd2), "PROSE");
        assert_eq!(t.dim, Color32::from_rgb(0x70, 0x7c, 0x70), "DIM");

        // conversation_view.rs — card states
        assert_eq!(t.running, Color32::from_rgb(0xe6, 0xc0, 0x4c), "RUNNING");
        assert_eq!(t.asking, Color32::from_rgb(0x7f, 0xb8, 0xe6), "ASKING");
        assert_eq!(t.ok, Color32::from_rgb(0x6f, 0xc2, 0x76), "OK");
        assert_eq!(t.bad, Color32::from_rgb(0xe0, 0x6c, 0x5f), "BAD");
        assert_eq!(t.surface_empty, Color32::from_rgb(0x0a, 0x0e, 0x0a), "SURFACE_EMPTY");

        // conversation_view.rs — the status strip
        assert_eq!(t.strip_fill, Color32::from_rgb(0x0b, 0x11, 0x0b), "STRIP_FILL");
        assert_eq!(t.strip_edge, Color32::from_rgb(0x22, 0x2c, 0x22), "STRIP_EDGE");
        assert_eq!(t.model_fill, Color32::from_rgb(0x15, 0x1e, 0x15), "MODEL_FILL");
        assert_eq!(t.model_edge, Color32::from_rgb(0x3a, 0x50, 0x3a), "MODEL_EDGE");
        assert_eq!(t.model_text, Color32::from_rgb(0xc6, 0xdf, 0xc6), "MODEL_TEXT");
        assert_eq!(t.model_badge, Color32::from_rgb(0x8a, 0xb0, 0x8a), "MODEL_BADGE");
        assert_eq!(t.mode_alert, Color32::from_rgb(0xd8, 0x9a, 0x5c), "MODE_ALERT");
        assert_eq!(t.mode_note, Color32::from_rgb(0x8a, 0xa6, 0xc2), "MODE_NOTE");
        assert_eq!(t.context_track, Color32::from_rgb(0x2c, 0x38, 0x2c), "CONTEXT_TRACK");
        assert_eq!(
            t.context_track_empty,
            Color32::from_rgb(0x1a, 0x22, 0x1a),
            "CONTEXT_TRACK_EMPTY"
        );
        assert_eq!(t.context_arc, Color32::from_rgb(0x5f, 0x93, 0xcc), "CONTEXT_ARC");
        // `CONTEXT_ARC_HIGH` was written `= MODE_ALERT`, so the pin is that it still equals
        // that amber *and* that it is a field of its own — the second half is what a
        // re-chosen alert colour will need.
        assert_eq!(t.context_arc_high, Color32::from_rgb(0xd8, 0x9a, 0x5c), "CONTEXT_ARC_HIGH");
        assert_eq!(t.context_arc_high, t.mode_alert, "CONTEXT_ARC_HIGH = MODE_ALERT, as shipped");

        // conversation_view.rs — the composer
        assert_eq!(t.composer_fill, Color32::from_rgb(0x0e, 0x14, 0x0e), "COMPOSER_FILL");
        assert_eq!(t.composer_edge, Color32::from_rgb(0x2b, 0x38, 0x2b), "COMPOSER_EDGE");
        assert_eq!(
            t.composer_edge_focus,
            Color32::from_rgb(0x4c, 0x7a, 0x52),
            "COMPOSER_EDGE_FOCUS"
        );
        assert_eq!(
            t.composer_edge_dead,
            Color32::from_rgb(0x3a, 0x2c, 0x2c),
            "COMPOSER_EDGE_DEAD"
        );

        // term_view.rs
        assert_eq!(t.term_bg, Color32::from_rgb(0x0a, 0x0d, 0x0a), "DEFAULT_BG");
        assert_eq!(t.term_fg, Color32::from_rgb(0xc8, 0xe6, 0xc8), "DEFAULT_FG");
        // The scrim was written inline as `from_rgba_unmultiplied(0x0a, 0x0d, 0x0a, scrim)`;
        // only the three colour channels moved here, and the alpha is still the env var's.
        assert_eq!(t.term_scrim_tint, Color32::from_rgb(0x0a, 0x0d, 0x0a), "the scrim's tint");
        assert_eq!(
            t.term_exited_notice,
            Color32::from_rgb(0x80, 0x8a, 0x80),
            "the [process exited] notice"
        );
        assert_eq!(
            t.ansi16,
            [
                Color32::from_rgb(0x10, 0x14, 0x10),
                Color32::from_rgb(0xcc, 0x52, 0x4b),
                Color32::from_rgb(0x5c, 0xb8, 0x5c),
                Color32::from_rgb(0xc2, 0xb0, 0x4c),
                Color32::from_rgb(0x56, 0x92, 0xd8),
                Color32::from_rgb(0xb0, 0x6c, 0xc0),
                Color32::from_rgb(0x4c, 0xb8, 0xb0),
                Color32::from_rgb(0xc8, 0xd2, 0xc8),
                Color32::from_rgb(0x50, 0x5a, 0x50),
                Color32::from_rgb(0xe8, 0x6a, 0x62),
                Color32::from_rgb(0x74, 0xd8, 0x74),
                Color32::from_rgb(0xdc, 0xcc, 0x66),
                Color32::from_rgb(0x74, 0xac, 0xec),
                Color32::from_rgb(0xcc, 0x88, 0xdc),
                Color32::from_rgb(0x68, 0xd4, 0xcc),
                Color32::from_rgb(0xee, 0xf4, 0xee),
            ],
            "ANSI16"
        );

        // block_panel.rs
        assert_eq!(
            t.panel_fill,
            Color32::from_rgba_premultiplied(0x0b, 0x12, 0x0e, 0xe6),
            "PANEL_FILL — premultiplied, alpha included"
        );
        assert_eq!(t.panel_edge, Color32::from_rgb(0x3e, 0x7a, 0x52), "PANEL_EDGE");
        assert_eq!(t.panel_title, Color32::from_rgb(0x8f, 0xe0, 0xa8), "PANEL_TITLE");
        // A panel's body text was `DEFAULT_FG`, imported across modules. Same bytes, own field.
        assert_eq!(t.panel_text, Color32::from_rgb(0xc8, 0xe6, 0xc8), "the panel body's text");

        // timeline.rs
        assert_eq!(
            t.timeline_scripted_fill,
            Color32::from_rgb(0x33, 0x2b, 0x12),
            "the scripted-demo banner"
        );
        assert_eq!(
            t.timeline_scripted_mark,
            Color32::from_rgb(0xe6, 0xc0, 0x4c),
            "the scripted-demo mark"
        );
        assert_eq!(t.timeline_status_pending, Color32::from_rgb(0xb0, 0xb0, 0xb0), "pending");
        assert_eq!(t.timeline_status_running, Color32::from_rgb(0xe6, 0xc0, 0x4c), "running");
        assert_eq!(t.timeline_status_ok, Color32::from_rgb(0x6f, 0xc2, 0x76), "ok");
        assert_eq!(t.timeline_status_failed, Color32::from_rgb(0xe0, 0x6c, 0x5f), "failed");
        assert_eq!(t.timeline_status_denied, Color32::from_rgb(0xe0, 0x6c, 0x5f), "denied");
        assert_eq!(t.timeline_status_cancelled, Color32::from_rgb(0xb0, 0xb0, 0xb0), "cancelled");
        assert_eq!(
            t.timeline_approval_accent,
            Color32::from_rgb(0xe6, 0xc0, 0x4c),
            "the approval card's accent"
        );
        assert_eq!(t.timeline_bubble_user, Color32::from_rgb(0x24, 0x33, 0x42), "the user bubble");
        assert_eq!(
            t.timeline_bubble_other,
            Color32::from_rgb(0x26, 0x26, 0x2b),
            "everyone else's bubble"
        );

        // tabs.rs, and the strip's own plate from shell_main.rs
        assert_eq!(t.tab_strip_fill, Color32::from_rgb(0x07, 0x09, 0x07), "the tab strip's fill");
        assert_eq!(t.tab_active, Color32::from_rgb(0xc8, 0xe6, 0xc8), "the active tab's title");
        assert_eq!(t.tab_inactive, Color32::from_rgb(0x60, 0x6c, 0x60), "an inactive tab's title");
        assert_eq!(t.tab_plus, Color32::from_rgb(0x8a, 0x96, 0x8a), "the + button");
        assert_eq!(t.tab_menu_fill, Color32::from_rgb(0x10, 0x14, 0x10), "the harness menu");
        assert_eq!(
            t.tab_menu_installed,
            Color32::from_rgb(0xc8, 0xe6, 0xc8),
            "an installed harness"
        );
        assert_eq!(t.tab_menu_missing, Color32::from_rgb(0x50, 0x5a, 0x50), "a missing harness");
    }

    /// ⚠️ **`card_left_rule` is deliberately NOT in the test above**, which pins values read
    /// out of `main` before the extraction: this field is new, so there is nothing on `main`
    /// to have read it from and adding it there would quietly weaken the one test that
    /// backs "the look did not change". Its claim is different and is stated here — the
    /// `organon` palette declines the rule, which is what makes the whole posture tier
    /// invisible at every `t`.
    #[test]
    fn the_organon_palette_declines_the_left_rule() {
        assert_eq!(Theme::organon().card_left_rule, Color32::TRANSPARENT);
        assert_eq!(Theme::organon().card_left_rule.a(), 0, "…and it is the ALPHA that decides");
    }

    /// The default *is* the shipped look, so a construction site that says nothing cannot
    /// silently get a different console.
    #[test]
    fn the_default_theme_is_organon() {
        assert_eq!(Theme::default(), Theme::organon());
    }

    /// 🚨 **The roles that share bytes today are separate fields, and this is the test that
    /// says so on purpose rather than by accident.** Merging any pair below would look like
    /// tidying and would quietly weld two decisions together: a second palette could then
    /// not lighten the terminal without lightening a human's typed line with it, or re-tune
    /// "worth acting on" without moving the context ring's warning.
    #[test]
    fn roles_that_share_a_value_are_still_separate_fields() {
        let t = Theme::organon();
        // #c8e6c8, four ways.
        for (name, value) in [
            ("term_fg", t.term_fg),
            ("human_text", t.human_text),
            ("tab_active", t.tab_active),
            ("tab_menu_installed", t.tab_menu_installed),
            ("panel_text", t.panel_text),
        ] {
            assert_eq!(value, Color32::from_rgb(0xc8, 0xe6, 0xc8), "{name}");
        }
        // The terminal's background and the scrim's tint: the same three channels, two
        // roles — one is what a cell with no colour is, the other is what dims the world
        // behind the glyphs.
        assert_eq!(t.term_bg, t.term_scrim_tint);
        // Amber, three ways: a running tool, the permission alert, the ring above its mark.
        assert_eq!(t.running, Color32::from_rgb(0xe6, 0xc0, 0x4c));
        assert_eq!(t.timeline_status_running, t.running);
        assert_eq!(t.context_arc_high, t.mode_alert);
        // Two different answers that happen to be the same red, and two that are the same
        // grey.
        assert_eq!(t.timeline_status_denied, t.timeline_status_failed);
        assert_eq!(t.timeline_status_cancelled, t.timeline_status_pending);
    }
}

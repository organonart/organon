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
//! `Theme` is held by the console's app state (`Console`, in `native/src/console_main.rs`) and
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
//! # What a second palette overrides
//!
//! Every field below, and only these. The console's remaining colour comes from two places
//! a palette still does not touch: the xterm 256-colour cube and greyscale ramp — which are
//! a **standard**, not a taste, and are computed rather than stored — and the truecolor and
//! OSC-override values a running program sends, which belong to that program.
//!
//! egui's own `Visuals` — widget plates, popup frames, the text-selection wash, scrollbars —
//! *was* the third, and is not any more: [`Theme::visuals`] derives it from the fields below.
//! Leaving it out was survivable while every palette was dark; it is not survivable at all
//! once one is light, because half the pixels in the window would stay hardcoded dark and the
//! light palette would read as broken rather than as light.
//!
//! # Four palettes, one default
//!
//! [`Theme::organon`] is the console's own look and is still the default; [`Theme::light`],
//! [`Theme::dark`] and [`Theme::chocolate`] are the three James specified. **Nothing selects
//! one yet** — [`Theme::by_name`] exists so a picker and `prefs.rs`'s stored `theme` name can
//! both resolve a string, and that is all.
//!
//! ⚠️ **The specs name about ten roles each; this struct has about fifty fields.** Every field
//! a spec does not reach is *derived*, and the rule is written at the site rather than left as
//! a hex nobody can argue with. The rules, in one place, so a fifth palette can follow them:
//!
//! 1. **The surface ladder.** A spec gives a page and a panel; the second raised step is the
//!    spec's own hairline colour, which is by construction exactly one step further from the
//!    page. Chocolate names all four of its steps and none of this applies to it.
//! 2. **States.** A state the spec names takes its named colour. A state it does not name is
//!    drawn from the palette's **text ladder**, never from a hue the spec never introduced —
//!    which is why none of the three has an amber, and why "a tool is running" is primary text
//!    rather than the orange `organon` uses.
//! 3. **The one exception to (2)**: "blocked on a human", the context ring, and the
//!    non-default-permission note all take the palette's accent. They are the roles the accent
//!    exists for — present, not an outcome, and the field docs already forbid drawing them red.
//! 4. **Tinted plates** (the user bubble, the scripted-replay banner) are a stated linear mix
//!    of a state colour into a surface, at the ratio written beside each one.
//!
//! ⚠️ **`ansi16` is CHOSEN for all three, not specified.** All three specs were written against
//! the conversation view, and none of them says anything about a terminal. See the comment on
//! each array.

use egui::Color32;

/// Where a palette's egui chrome comes from.
///
/// Not a colour and not a form token: it answers "which of the two egui bases does
/// [`Theme::visuals`] start from, and does it derive at all". Three answers rather than a
/// bool, because [`Theme::organon`] needs a third: **its chrome is `egui::Visuals::dark()`
/// byte-for-byte**, which is what `console_main.rs` has called since the console's first frame.
/// Deriving `organon`'s chrome from its own fields would silently restyle the shipped console
/// while adding new palettes, so the shipped palette says "not derived" and
/// `organon_chrome_is_still_egui_dark_to_the_byte` holds it there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeSource {
    /// egui's built-in dark chrome, unmodified — what the console has always painted.
    EguiDark,
    /// Derived from the palette's own fields, over egui's dark base.
    DerivedDark,
    /// Derived from the palette's own fields, over egui's light base.
    DerivedLight,
}

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
    /// The legibility scrim's tint — the three colour channels only. The **alpha** a given
    /// launch paints is `ORGANON_SHELL_SCRIM`'s, floored by [`Theme::scrim_floor`] through
    /// [`crate::term_view::scrim_alpha`].
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

    // ── Not pigment: the two things a palette decides that are not a colour ──
    /// **The floor the legibility scrim may never go below**, as an alpha byte — PRD §4.6's
    /// inviolable half, now asked of the palette rather than of a single constant.
    ///
    /// 🚨 **The rule is "the glyphs stay legible", and it always was. What moved is the
    /// assumption that legibility means *darkness*.** A scrim is `term_scrim_tint` laid over
    /// the live backdrop, and the floor is how much of the backdrop it is allowed to leave
    /// showing. `96` — the number PRD §4.6 fixed — protects pale glyphs on a near-black page
    /// by keeping a bright frame from washing them out. Laid over a **light** page it protects
    /// nothing at all: the glyphs are dark, the danger is a *dark* backdrop, and 96/255 of
    /// white over a dark scene leaves a mid-grey that dark text disappears into. A light theme
    /// is therefore not reachable by swapping colours — without this field it would sit under a
    /// compulsory near-black veil forever.
    ///
    /// ⚠️ **It is still un-overridable by a setting, which is the half that must not weaken.**
    /// [`crate::term_view::scrim_alpha`] takes this value and clamps `ORGANON_SHELL_SCRIM` up
    /// to it; no environment variable, in range or out of it, can cross it. What changed is
    /// *who names it*: the palette, which is compiled in, rather than user configuration. A
    /// palette is a whole coherent instrument including the terms on which its glyphs stay
    /// readable; a scrim value typed into a shim is a preference about one of them.
    pub scrim_floor: u8,
    /// Where this palette's egui chrome comes from — see [`ChromeSource`] and
    /// [`Theme::visuals`].
    pub chrome: ChromeSource,
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

            // PRD §4.6's number, unchanged: this is the palette it was chosen against.
            scrim_floor: crate::term_view::SCRIM_FLOOR,
            // Not derived, deliberately — `console_main.rs` has called `Visuals::dark()` since
            // the console's first frame and this palette must keep painting exactly that.
            chrome: ChromeSource::EguiDark,
        }
    }

    /// 🚨 **[`Theme::light`]'s four surface steps, and the palette's deliberate departure from
    /// the spec James wrote.** The spec's ladder is `#ffffff` → `#f7f8f9` → `#e2e5e9` →
    /// `#c9ced6`. All four have moved, **twice**, and both moves are instructions rather than
    /// drift:
    ///
    /// 1. **2026-08-14, the page alone** — `#ffffff` → `#fafbfc`. *"turn down the value slash
    ///    brightness of the white, that is the whitest white"*.
    /// 2. **2026-08-14, later, the whole ladder** — `#fafbfc` → `#d7d8d9` and the other three
    ///    steps with it. *"the white part is too white. Move it down to about a 0.85 V in the
    ///    HSV system"*.
    ///
    /// **The second is a correction of the first, not a contradiction of it.** He looked at
    /// `#fafbfc` running and it was still too bright; `CONSOLE_ARCHITECTURE.md` §3's ledger had
    /// already recorded that 1.18 % "may not be enough", and it was not.
    ///
    /// 🚨 **The result is a light GREY page, not a white one, and that is what was asked for.**
    /// `V = 0.851` is a page you would describe as pale grey card stock. Do not "fix" it back
    /// toward white because it stopped looking like paper — it stopped on purpose.
    ///
    /// # Why all four had to move together
    ///
    /// The ladder climbs *away* from the page by darkening, and the page→panel step is the
    /// whisper of the four: **3/3/3 per channel** against panel→hairline's 21/19/16 and
    /// hairline→strong's 25/23/19. Dropping the page to `V ≈ 0.85` alone puts it **30 units
    /// below the panel** and inverts the ladder — every plate drawn on the page (the composer,
    /// the tab strip, a bubble) would read as *raised out of* the paper instead of recessed
    /// into it, which is the opposite of the printed-publication metaphor.
    /// `the_light_page_stays_a_step_above_the_panel` fails on exactly that.
    ///
    /// So the move is a **uniform −35 on every channel of every step**, which preserves all
    /// three inter-step distances to the unit and preserves each step's own cool tilt. `−35`
    /// is what takes the page's top channel from `252` to `217`; `217/255 = 0.8510`, i.e.
    /// 0.1 % off the value asked for, and the nearest a `u8` can get to `0.85 × 255 = 216.75`.
    ///
    /// ⚠️ **Uniform subtraction does not weaken the ladder — it strengthens it slightly.**
    /// sRGB's transfer curve makes an equal code-value step a larger luminance ratio lower
    /// down, so measured WCAG contrast between adjacent steps *rises*: panel-on-page
    /// 1.026 → 1.030, hairline-on-page 1.220 → 1.253, strong-on-page 1.526 → 1.617. The floor
    /// is fine; `strong` at `#a6abb3` is a more visible border than `#c9ced6` was, not a
    /// mid-grey one.
    ///
    /// ⚠️ **What the move genuinely costs is the TEXT ladder, and that ladder deliberately did
    /// not move.** `primary #0f1114` is untouched at 13.3:1 on the new page, but the roles
    /// James specified as *faint* and *secondary* lose most of what they had: `dim #8b919b`
    /// falls 3.06 → **2.22** on the page and 2.51 → **1.77** on a hairline plate, and
    /// `secondary #5d636c` falls 5.70 → **4.12** on the panel, crossing under AA's 4.5. There
    /// is no compression of the *surface* ladder that fixes this — those numbers are set by
    /// the page James asked to lower against text he did not ask to darken, so the only repair
    /// is to darken the text ladder, which is his call and not this change's.
    /// `CONSOLE_ARCHITECTURE.md` §3 carries it. (`#737983`, a uniform −24, is what would
    /// restore `dim` to its old 3.06 — recorded so the option is costed, not taken.)
    ///
    /// ⚠️ **Four fields are *functions* of these steps and moved with them** — module rule 4.
    /// [`Theme::panel_fill`] is the page premultiplied at `0xe6`, [`Theme::composer_edge_dead`]
    /// is the error colour 1:1 into the page, [`Theme::timeline_scripted_fill`] is the error at
    /// 20 % over the page, and [`Theme::timeline_bubble_user`] is the accent 1:5 over the
    /// panel. **The latter two had already gone stale**: they were computed against the spec's
    /// `#ffffff` and the 2026-08-14 page move left them behind, so a written derivation had
    /// been quietly false for a day. Both are recomputed here against the values that are
    /// actually in the struct.
    ///
    /// ⚠️ **These are constants rather than repeated literals for the reason this entry
    /// exists.** The panel is five fields, the hairline six, the strong border four; a third
    /// correction spelled as fifteen hand-edited hexes is a correction that lands on fourteen
    /// of them. Sharing a constant does **not** weld roles together — every field below still
    /// assigns independently and a fifth palette can part any of them — it only stops one
    /// *step* of one ladder being two different colours by accident.
    ///
    /// ⚠️ **[`Theme::term_bg`] and [`Theme::term_scrim_tint`] both take the page, and they must
    /// track.** They are two roles — what an unpainted cell *is*, and what dims the live
    /// backdrop behind the glyphs — but the scrim is laid over the world at up to
    /// [`crate::term_view::SCRIM_FLOOR_LIGHT`], so a scrim left brighter than the page would
    /// keep painting the exact glare being removed, over a *larger* area than the terminal, and
    /// would make a scrimmed region read brighter than the page it borders. The field split
    /// stays (the struct never merges roles by value); the value is stated once so the two
    /// cannot drift apart by accident.
    ///
    /// ⚠️ **Nobody has seen this ladder on James's display either.** `#fafbfc` was reasoned and
    /// turned out to be wrong; `#d7d8d9` is arithmetic against a number he named, which is a
    /// better starting point and still not an observation. `CONSOLE_ARCHITECTURE.md` §3's
    /// ledger carries it, and only he can close it.
    const LIGHT_PAGE: Color32 = Color32::from_rgb(0xd7, 0xd8, 0xd9);
    /// The ladder's second step — the spec's `#f7f8f9` less the uniform 35. See
    /// [`Theme::LIGHT_PAGE`] for the whole ladder and why it moved.
    const LIGHT_PANEL: Color32 = Color32::from_rgb(0xd4, 0xd5, 0xd6);
    /// The third step — the spec's `#e2e5e9` less the uniform 35. Module rule 1's "second
    /// raised step": a bubble sitting *on* a panel needs one more step past it.
    const LIGHT_HAIRLINE: Color32 = Color32::from_rgb(0xbf, 0xc2, 0xc6);
    /// The fourth and strongest step — the spec's `#c9ced6` less the uniform 35. The border
    /// weight, not a surface: nothing is filled with it except a pressed widget's plate.
    const LIGHT_STRONG: Color32 = Color32::from_rgb(0xa6, 0xab, 0xb3);

    /// **A printed technical publication** — quiet, typographic, no phosphor green anywhere.
    /// Specified by James; the ten roles he named are marked `[spec]` below and everything
    /// else follows the module's derivation rules.
    ///
    /// ⚠️ **"No green anywhere" and a green success colour are both in the spec, and they are
    /// not in conflict.** The prohibition is on green as *structure* — the console's phosphor
    /// text, borders and terminal foreground, which is what makes `organon` look like a
    /// terminal rather than a page. `#1a6b46` is a named **state**, and a publication that
    /// could not say "this succeeded" in green would be paying for the rule twice.
    ///
    /// 🚨 **The four surface roles no longer hold their specified hexes: the whole ladder was
    /// moved down, twice, on James's instruction.** See [`Theme::LIGHT_PAGE`] for what they are
    /// now, the arithmetic, and what it cost. Every `[spec]` **text and state** value below —
    /// primary, secondary, faint, accent, success, error — is untouched.
    pub const fn light() -> Self {
        // [spec] ⚠️ FOUR ROWS SUPERSEDED — the whole surface ladder, uniformly −35 per channel.
        //        See `LIGHT_PAGE`/`LIGHT_PANEL`/`LIGHT_HAIRLINE`/`LIGHT_STRONG` for the two
        //        instructions (James, 2026-08-14: the page, then the ladder to V ≈ 0.85).
        //
        //        page     #ffffff  → SUPERSEDED → #d7d8d9   (V 1.000 → 0.851)
        //        panel    #f7f8f9  → SUPERSEDED → #d4d5d6   (V 0.976 → 0.839)
        //        hairline #e2e5e9  → SUPERSEDED → #bfc2c6   (V 0.914 → 0.776)
        //        strong   #c9ced6  → SUPERSEDED → #a6abb3   (V 0.839 → 0.702)
        //
        //        primary #0f1114 · secondary #5d636c · faint #8b919b · accent #1440c4 ·
        //        success #1a6b46 · error #a32020        ← all six still exactly as specified
        //
        // Derived, rule 1: the ladder's second raised step is the spec's hairline — the spec
        // names one panel, and a bubble sitting *on* a panel needs a step past it.
        Self {
            // "a single 2px vertical hairline rule on its left edge in #c9ced6, no outline
            // on the other three sides" — the stronger border, and the reason this palette
            // reads as a printed page rather than a boxed one. The *weight* is what the spec
            // names; the hex is that weight carried down the ladder with everything else.
            card_left_rule: Self::LIGHT_STRONG,
            human_text: Color32::from_rgb(0x0f, 0x11, 0x14),
            human_fill: Self::LIGHT_HAIRLINE,
            prose: Color32::from_rgb(0x0f, 0x11, 0x14),
            dim: Color32::from_rgb(0x8b, 0x91, 0x9b),

            // Rule 2: no amber is specified, so "in flight" is primary text — present and
            // unmissable without introducing a hue the publication never uses.
            running: Color32::from_rgb(0x0f, 0x11, 0x14),
            asking: Color32::from_rgb(0x14, 0x40, 0xc4),
            ok: Color32::from_rgb(0x1a, 0x6b, 0x46),
            bad: Color32::from_rgb(0xa3, 0x20, 0x20),
            surface_empty: Self::LIGHT_PANEL,

            strip_fill: Self::LIGHT_PANEL,
            strip_edge: Self::LIGHT_HAIRLINE,
            model_fill: Self::LIGHT_HAIRLINE,
            model_edge: Self::LIGHT_STRONG,
            model_text: Color32::from_rgb(0x0f, 0x11, 0x14),
            model_badge: Color32::from_rgb(0x5d, 0x63, 0x6c),
            // Rule 3: accent, not the error red the field doc forbids and not an amber this
            // palette does not have.
            mode_alert: Color32::from_rgb(0x14, 0x40, 0xc4),
            mode_note: Color32::from_rgb(0x5d, 0x63, 0x6c),
            context_track: Self::LIGHT_STRONG,
            // Fainter than the track and still not the band it sits on — on a light page
            // "fainter" means *closer to the page*, which inverts the ordering `organon` has.
            context_track_empty: Self::LIGHT_HAIRLINE,
            context_arc: Color32::from_rgb(0x14, 0x40, 0xc4),
            // The arc above the high-water mark is the one reading on the band that becomes a
            // failure if it is ignored, so it takes the error colour rather than `organon`'s
            // amber. It is deliberately no longer equal to `mode_alert`.
            context_arc_high: Color32::from_rgb(0xa3, 0x20, 0x20),

            composer_fill: Self::LIGHT_PANEL,
            composer_edge: Self::LIGHT_HAIRLINE,
            composer_edge_focus: Color32::from_rgb(0x14, 0x40, 0xc4),
            // Rule 4: the error colour mixed 1:1 into the page — a dead composer is a failure
            // worn at the weight of a border, not shouted.
            //
            // ⚠️ **This was stale and is now recomputed.** It was `#d19090`, which is the 1:1
            // mix against the spec's `#ffffff`; the 2026-08-14 page move to `#fafbfc` did not
            // carry it, so the stated derivation had been false since. Against `LIGHT_PAGE`:
            // (163+215)/2 = 189 → 0xbd, (32+216)/2 = 124 → 0x7c, (32+217)/2 = 124.5 → 0x7d.
            composer_edge_dead: Color32::from_rgb(0xbd, 0x7c, 0x7d),

            // Both `LIGHT_PAGE`, stated once — see its doc for the deviation and the
            // ladder arithmetic that bounds it.
            term_bg: Self::LIGHT_PAGE,
            term_fg: Color32::from_rgb(0x0f, 0x11, 0x14),
            term_scrim_tint: Self::LIGHT_PAGE,
            term_exited_notice: Color32::from_rgb(0x5d, 0x63, 0x6c),
            // ⚠️ **CHOSEN, not specified.** The spec is silent on the terminal, and a light
            // background changes what the sixteen names can mean: "white" and "bright white"
            // have to become dark or the text vanishes, and every hue has to darken to hold
            // contrast against paper. These follow the GitHub Light lineage, which is the
            // most widely-read light terminal palette and therefore the one a TUI's own
            // colour scheme is most likely to have been checked against. Nobody has read a
            // TUI through them.
            ansi16: [
                Color32::from_rgb(0x24, 0x29, 0x2e), // black
                Color32::from_rgb(0xd1, 0x24, 0x2f), // red
                Color32::from_rgb(0x1a, 0x7f, 0x37), // green
                Color32::from_rgb(0x9a, 0x67, 0x00), // yellow
                Color32::from_rgb(0x09, 0x69, 0xda), // blue
                Color32::from_rgb(0x82, 0x50, 0xdf), // magenta
                Color32::from_rgb(0x1b, 0x7c, 0x83), // cyan
                Color32::from_rgb(0x6e, 0x77, 0x81), // white — grey, or it is invisible
                Color32::from_rgb(0x57, 0x60, 0x6a), // bright black
                Color32::from_rgb(0xa4, 0x0e, 0x26), // bright red
                Color32::from_rgb(0x11, 0x63, 0x29), // bright green
                Color32::from_rgb(0x7d, 0x4e, 0x00), // bright yellow
                Color32::from_rgb(0x05, 0x50, 0xae), // bright blue
                Color32::from_rgb(0x66, 0x39, 0xba), // bright magenta
                Color32::from_rgb(0x1b, 0x7c, 0x83), // bright cyan
                Color32::from_rgb(0x24, 0x29, 0x2e), // bright white — the darkest, deliberately
            ],

            // Rule 4: the page at the same 0xe6 alpha `organon` uses, premultiplied.
            //
            // ⚠️ **This moved with the page and had to.** Rule 4 states this field as a
            // *function* of the page, so leaving `0xe6,0xe6,0xe6` behind would not have been
            // "not changing a patch panel" — it would have been a stated derivation quietly
            // becoming false, which is the one failure a written rule is supposed to prevent.
            // `LIGHT_PAGE` × 0xe6/0xff, rounded: 215·230/255 = 193.9 → 0xc2, 216 → 194.8 →
            // 0xc3, 217 → 195.7 → 0xc4. Hand-computed because `from_rgba_premultiplied` takes
            // the product, and this is a `const fn`.
            panel_fill: Color32::from_rgba_premultiplied(0xc2, 0xc3, 0xc4, 0xe6),
            panel_edge: Self::LIGHT_STRONG,
            panel_title: Color32::from_rgb(0x0f, 0x11, 0x14),
            panel_text: Color32::from_rgb(0x0f, 0x11, 0x14),

            // Rule 4: the error colour at 20 % over the page. A replay must never pass as a
            // live agent, so the banner is drawn from the palette's warning family rather than
            // from its accent.
            //
            // ⚠️ **Stale for the same reason `composer_edge_dead` was, and recomputed with it.**
            // `#edd2d2` is 20 % error over `#ffffff`; the page had moved twice since. Against
            // `LIGHT_PAGE`: 0.2·163 + 0.8·215 = 204.6 → 0xcd, 0.2·32 + 0.8·216 = 179.2 → 0xb3,
            // 0.2·32 + 0.8·217 = 180.0 → 0xb4. A banner left at `#edd2d2` would have glowed
            // brighter than the page it interrupts, which is the glare being removed.
            timeline_scripted_fill: Color32::from_rgb(0xcd, 0xb3, 0xb4),
            timeline_scripted_mark: Color32::from_rgb(0xa3, 0x20, 0x20),
            timeline_status_pending: Color32::from_rgb(0x8b, 0x91, 0x9b),
            timeline_status_running: Color32::from_rgb(0x0f, 0x11, 0x14),
            timeline_status_ok: Color32::from_rgb(0x1a, 0x6b, 0x46),
            timeline_status_failed: Color32::from_rgb(0xa3, 0x20, 0x20),
            timeline_status_denied: Color32::from_rgb(0xa3, 0x20, 0x20),
            timeline_status_cancelled: Color32::from_rgb(0x8b, 0x91, 0x9b),
            timeline_approval_accent: Color32::from_rgb(0x14, 0x40, 0xc4),
            // Rule 4: the accent at 1:5 over the panel — a tinted plate, not a coloured one.
            // Recomputed because the panel moved: (20 + 5·212)/6 = 180 → 0xb4,
            // (64 + 5·213)/6 = 188.2 → 0xbc, (196 + 5·214)/6 = 211 → 0xd3. This one was not
            // stale before — the earlier correction moved the page and left the panel alone —
            // but it would be now, and it is also `visuals().selection.bg_fill`, so a plate
            // left at `#d1d9f0` would make every text selection brighter than the page.
            timeline_bubble_user: Color32::from_rgb(0xb4, 0xbc, 0xd3),
            timeline_bubble_other: Self::LIGHT_PANEL,

            tab_strip_fill: Self::LIGHT_PANEL,
            tab_active: Color32::from_rgb(0x0f, 0x11, 0x14),
            tab_inactive: Color32::from_rgb(0x5d, 0x63, 0x6c),
            tab_plus: Color32::from_rgb(0x5d, 0x63, 0x6c),
            tab_menu_fill: Self::LIGHT_HAIRLINE,
            tab_menu_installed: Color32::from_rgb(0x0f, 0x11, 0x14),
            tab_menu_missing: Color32::from_rgb(0x8b, 0x91, 0x9b),

            scrim_floor: crate::term_view::SCRIM_FLOOR_LIGHT,
            chrome: ChromeSource::DerivedLight,
        }
    }

    /// **The same publication as a photographic negative** — calm, low contrast, never pure
    /// black. Every derivation is [`Theme::light`]'s, applied to the inverted ladder; read the
    /// comments there for the reasoning rather than having it restated twice.
    pub const fn dark() -> Self {
        // [spec] page #0f1114 · panel #16181c · primary #e6e9ed · secondary #9aa1ab ·
        //        faint #6b727c · hairline #24272d · strong #363b43 · accent #8fb0ff ·
        //        success #6fbf95 · error #e88b8b
        //
        // Rule 1: the second raised step is the hairline `#24272d`.
        Self {
            // The same rule as `light`, developed as its negative: the stronger border,
            // `#363b43`. Same form, inverted palette — that is what makes these two one
            // design rather than two.
            card_left_rule: Color32::from_rgb(0x36, 0x3b, 0x43),
            human_text: Color32::from_rgb(0xe6, 0xe9, 0xed),
            human_fill: Color32::from_rgb(0x24, 0x27, 0x2d),
            prose: Color32::from_rgb(0xe6, 0xe9, 0xed),
            dim: Color32::from_rgb(0x6b, 0x72, 0x7c),

            running: Color32::from_rgb(0xe6, 0xe9, 0xed),
            asking: Color32::from_rgb(0x8f, 0xb0, 0xff),
            ok: Color32::from_rgb(0x6f, 0xbf, 0x95),
            bad: Color32::from_rgb(0xe8, 0x8b, 0x8b),
            surface_empty: Color32::from_rgb(0x16, 0x18, 0x1c),

            strip_fill: Color32::from_rgb(0x16, 0x18, 0x1c),
            strip_edge: Color32::from_rgb(0x24, 0x27, 0x2d),
            model_fill: Color32::from_rgb(0x24, 0x27, 0x2d),
            model_edge: Color32::from_rgb(0x36, 0x3b, 0x43),
            model_text: Color32::from_rgb(0xe6, 0xe9, 0xed),
            model_badge: Color32::from_rgb(0x9a, 0xa1, 0xab),
            mode_alert: Color32::from_rgb(0x8f, 0xb0, 0xff),
            mode_note: Color32::from_rgb(0x9a, 0xa1, 0xab),
            context_track: Color32::from_rgb(0x36, 0x3b, 0x43),
            context_track_empty: Color32::from_rgb(0x24, 0x27, 0x2d),
            context_arc: Color32::from_rgb(0x8f, 0xb0, 0xff),
            context_arc_high: Color32::from_rgb(0xe8, 0x8b, 0x8b),

            composer_fill: Color32::from_rgb(0x16, 0x18, 0x1c),
            composer_edge: Color32::from_rgb(0x24, 0x27, 0x2d),
            composer_edge_focus: Color32::from_rgb(0x8f, 0xb0, 0xff),
            // Rule 4: error mixed 1:1 into the page.
            composer_edge_dead: Color32::from_rgb(0x7c, 0x4e, 0x50),

            term_bg: Color32::from_rgb(0x0f, 0x11, 0x14),
            term_fg: Color32::from_rgb(0xe6, 0xe9, 0xed),
            term_scrim_tint: Color32::from_rgb(0x0f, 0x11, 0x14),
            term_exited_notice: Color32::from_rgb(0x9a, 0xa1, 0xab),
            // ⚠️ **CHOSEN, not specified** — the spec says nothing about a terminal. Built to
            // this palette's own brief rather than borrowed: the six hues are the spec's own
            // accent/success/error plus a yellow, magenta and cyan mixed to the same muted
            // weight, so a TUI's colours sit at the page's contrast instead of shouting over
            // it, and the greys are the palette's own text ladder. Nobody has read a TUI
            // through them.
            ansi16: [
                Color32::from_rgb(0x24, 0x27, 0x2d), // black — the hairline
                Color32::from_rgb(0xe8, 0x8b, 0x8b), // red — the spec's error
                Color32::from_rgb(0x6f, 0xbf, 0x95), // green — the spec's success
                Color32::from_rgb(0xd9, 0xb2, 0x6f), // yellow
                Color32::from_rgb(0x8f, 0xb0, 0xff), // blue — the spec's accent
                Color32::from_rgb(0xc9, 0x9b, 0xdb), // magenta
                Color32::from_rgb(0x7f, 0xc9, 0xc9), // cyan
                Color32::from_rgb(0x9a, 0xa1, 0xab), // white — secondary text
                Color32::from_rgb(0x6b, 0x72, 0x7c), // bright black — faint text
                Color32::from_rgb(0xf0, 0xa3, 0xa3), // bright red
                Color32::from_rgb(0x8a, 0xd4, 0xac), // bright green
                Color32::from_rgb(0xe8, 0xc9, 0x8a), // bright yellow
                Color32::from_rgb(0xa9, 0xc3, 0xff), // bright blue
                Color32::from_rgb(0xd9, 0xb3, 0xe8), // bright magenta
                Color32::from_rgb(0x9a, 0xdc, 0xdc), // bright cyan
                Color32::from_rgb(0xe6, 0xe9, 0xed), // bright white — primary text
            ],

            panel_fill: Color32::from_rgba_premultiplied(0x0e, 0x0f, 0x12, 0xe6),
            panel_edge: Color32::from_rgb(0x36, 0x3b, 0x43),
            panel_title: Color32::from_rgb(0xe6, 0xe9, 0xed),
            panel_text: Color32::from_rgb(0xe6, 0xe9, 0xed),

            timeline_scripted_fill: Color32::from_rgb(0x3a, 0x29, 0x2c),
            timeline_scripted_mark: Color32::from_rgb(0xe8, 0x8b, 0x8b),
            timeline_status_pending: Color32::from_rgb(0x6b, 0x72, 0x7c),
            timeline_status_running: Color32::from_rgb(0xe6, 0xe9, 0xed),
            timeline_status_ok: Color32::from_rgb(0x6f, 0xbf, 0x95),
            timeline_status_failed: Color32::from_rgb(0xe8, 0x8b, 0x8b),
            timeline_status_denied: Color32::from_rgb(0xe8, 0x8b, 0x8b),
            timeline_status_cancelled: Color32::from_rgb(0x6b, 0x72, 0x7c),
            timeline_approval_accent: Color32::from_rgb(0x8f, 0xb0, 0xff),
            timeline_bubble_user: Color32::from_rgb(0x2a, 0x31, 0x42),
            timeline_bubble_other: Color32::from_rgb(0x16, 0x18, 0x1c),

            tab_strip_fill: Color32::from_rgb(0x16, 0x18, 0x1c),
            tab_active: Color32::from_rgb(0xe6, 0xe9, 0xed),
            tab_inactive: Color32::from_rgb(0x9a, 0xa1, 0xab),
            tab_plus: Color32::from_rgb(0x9a, 0xa1, 0xab),
            tab_menu_fill: Color32::from_rgb(0x24, 0x27, 0x2d),
            tab_menu_installed: Color32::from_rgb(0xe6, 0xe9, 0xed),
            tab_menu_missing: Color32::from_rgb(0x6b, 0x72, 0x7c),

            // A dark page, so PRD §4.6's own number applies unchanged.
            scrim_floor: crate::term_view::SCRIM_FLOOR,
            chrome: ChromeSource::DerivedDark,
        }
    }

    /// **A working desktop instrument** — warm graphite, overwhelmingly monochrome, colour as
    /// functional punctuation.
    ///
    /// 🚨 **The accent discipline is part of the spec, not decoration.** Green appears once, as
    /// the tiny state indicator; blue marks one selected semantic label; red is errors alone.
    /// Two consequences are visible below and neither is an oversight: [`Theme::ok`] is the
    /// **secondary grey**, because that field colours the literal word `ok` beside a finished
    /// tool call and the spec says that word is not green; and there is no amber anywhere, so
    /// "running" is primary text (module rule 2).
    ///
    /// ⚠️ **The green the spec does name lives on [`Theme::timeline_status_ok`]**, the nearest
    /// thing the current field set has to "a tiny state indicator". The console draws the word
    /// and its marker from **one** field, so the spec's word/dot split cannot be expressed
    /// without a new field and a draw-site change — out of scope for a palette-only change, and
    /// reported rather than fudged.
    ///
    /// ⚠️ **Never a cool blue-grey.** Every neutral below has its three channels equal, so no
    /// dark surface can pick up a blue cast; the ladder `#191919 → #1F1F1F → #262626 → #303030`
    /// is the palette's spine and a fifth step would have to be argued, not interpolated.
    pub const fn chocolate() -> Self {
        // [spec] page #191919 · nested code/output + input #1F1F1F · raised card #262626 ·
        //        raised/tab surface #303030 · hairline #3A3A3A · primary #EDEDED ·
        //        secondary #8F8F8F · muted #6E6E6E · success #4CC76E · accent #6699FF ·
        //        error #F0655E
        Self {
            // 🚨 **Transparent, and that is the whole point of an alpha rather than an enum.**
            // Chocolate's spec is "remove every prominent four-sided outline and unnecessary
            // hairline rule — separation should come from surface tone and restrained rounded
            // corners". So at desktop posture its box fades out and nothing fades in: the
            // `#262626` card against the `#191919` page IS the separation. A palette declines
            // the rule by declining to colour it, and no draw site learns a second shape.
            card_left_rule: Color32::TRANSPARENT,
            human_text: Color32::from_rgb(0xed, 0xed, 0xed),
            human_fill: Color32::from_rgb(0x26, 0x26, 0x26),
            prose: Color32::from_rgb(0xed, 0xed, 0xed),
            dim: Color32::from_rgb(0x6e, 0x6e, 0x6e),

            running: Color32::from_rgb(0xed, 0xed, 0xed),
            asking: Color32::from_rgb(0x66, 0x99, 0xff),
            // [spec] the word `ok` is the secondary grey, not the green.
            ok: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            bad: Color32::from_rgb(0xf0, 0x65, 0x5e),
            surface_empty: Color32::from_rgb(0x1f, 0x1f, 0x1f),

            strip_fill: Color32::from_rgb(0x1f, 0x1f, 0x1f),
            strip_edge: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            model_fill: Color32::from_rgb(0x26, 0x26, 0x26),
            model_edge: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            model_text: Color32::from_rgb(0xed, 0xed, 0xed),
            model_badge: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            mode_alert: Color32::from_rgb(0x66, 0x99, 0xff),
            mode_note: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            context_track: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            context_track_empty: Color32::from_rgb(0x26, 0x26, 0x26),
            context_arc: Color32::from_rgb(0x66, 0x99, 0xff),
            context_arc_high: Color32::from_rgb(0xf0, 0x65, 0x5e),

            composer_fill: Color32::from_rgb(0x1f, 0x1f, 0x1f),
            composer_edge: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            composer_edge_focus: Color32::from_rgb(0x66, 0x99, 0xff),
            // Rule 4: error mixed 1:1 into the page.
            composer_edge_dead: Color32::from_rgb(0x85, 0x3f, 0x3c),

            term_bg: Color32::from_rgb(0x19, 0x19, 0x19),
            term_fg: Color32::from_rgb(0xed, 0xed, 0xed),
            term_scrim_tint: Color32::from_rgb(0x19, 0x19, 0x19),
            term_exited_notice: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            // ⚠️ **CHOSEN, not specified**, and the one place this palette's monochrome
            // discipline is deliberately not applied: a program asking for ANSI red is asking
            // for red, and rendering `git diff` in graphite would be the console overpainting
            // what a program said. The accent/success/error trio is the spec's own; the other
            // three hues and their bright pairs are matched to that weight. Nobody has read a
            // TUI through them.
            ansi16: [
                Color32::from_rgb(0x30, 0x30, 0x30), // black — the raised surface
                Color32::from_rgb(0xf0, 0x65, 0x5e), // red — the spec's error
                Color32::from_rgb(0x4c, 0xc7, 0x6e), // green — the spec's success
                Color32::from_rgb(0xd9, 0xa4, 0x41), // yellow
                Color32::from_rgb(0x66, 0x99, 0xff), // blue — the spec's accent
                Color32::from_rgb(0xb9, 0x8b, 0xe0), // magenta
                Color32::from_rgb(0x5f, 0xc9, 0xc9), // cyan
                Color32::from_rgb(0x8f, 0x8f, 0x8f), // white — secondary text
                Color32::from_rgb(0x6e, 0x6e, 0x6e), // bright black — muted text
                Color32::from_rgb(0xf5, 0x8a, 0x84), // bright red
                Color32::from_rgb(0x74, 0xd9, 0x8f), // bright green
                Color32::from_rgb(0xe6, 0xbc, 0x6b), // bright yellow
                Color32::from_rgb(0x8f, 0xb4, 0xff), // bright blue
                Color32::from_rgb(0xcb, 0xa8, 0xeb), // bright magenta
                Color32::from_rgb(0x85, 0xda, 0xda), // bright cyan
                Color32::from_rgb(0xed, 0xed, 0xed), // bright white — primary text
            ],

            panel_fill: Color32::from_rgba_premultiplied(0x17, 0x17, 0x17, 0xe6),
            panel_edge: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            panel_title: Color32::from_rgb(0xed, 0xed, 0xed),
            panel_text: Color32::from_rgb(0xed, 0xed, 0xed),

            timeline_scripted_fill: Color32::from_rgb(0x44, 0x28, 0x27),
            timeline_scripted_mark: Color32::from_rgb(0xf0, 0x65, 0x5e),
            timeline_status_pending: Color32::from_rgb(0x6e, 0x6e, 0x6e),
            timeline_status_running: Color32::from_rgb(0xed, 0xed, 0xed),
            // The one green in the palette — see the ⚠️ on this constructor.
            timeline_status_ok: Color32::from_rgb(0x4c, 0xc7, 0x6e),
            timeline_status_failed: Color32::from_rgb(0xf0, 0x65, 0x5e),
            timeline_status_denied: Color32::from_rgb(0xf0, 0x65, 0x5e),
            timeline_status_cancelled: Color32::from_rgb(0x6e, 0x6e, 0x6e),
            timeline_approval_accent: Color32::from_rgb(0x66, 0x99, 0xff),
            timeline_bubble_user: Color32::from_rgb(0x2b, 0x33, 0x44),
            timeline_bubble_other: Color32::from_rgb(0x26, 0x26, 0x26),

            tab_strip_fill: Color32::from_rgb(0x30, 0x30, 0x30),
            tab_active: Color32::from_rgb(0xed, 0xed, 0xed),
            tab_inactive: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            tab_plus: Color32::from_rgb(0x8f, 0x8f, 0x8f),
            tab_menu_fill: Color32::from_rgb(0x30, 0x30, 0x30),
            tab_menu_installed: Color32::from_rgb(0xed, 0xed, 0xed),
            tab_menu_missing: Color32::from_rgb(0x6e, 0x6e, 0x6e),

            scrim_floor: crate::term_view::SCRIM_FLOOR,
            chrome: ChromeSource::DerivedDark,
        }
    }

    /// Every palette's name, in the order a picker should offer them — the default first.
    ///
    /// One list rather than a `match` arm each, so [`Theme::by_name`] and anything that wants
    /// to *enumerate* the palettes cannot disagree about which exist. A fifth palette is a row
    /// here and a row in `by_name`, and `every_name_resolves` fails if only one of them is
    /// written.
    pub const NAMES: [&'static str; 4] = ["organon", "light", "dark", "chocolate"];

    /// Resolve a stored or typed palette name.
    ///
    /// `None` for a name this build does not have — **never a panic, and never a substitute
    /// palette**, because the two callers want different things from a miss. A picker wants to
    /// say the name is unknown; `prefs.rs` (whose `theme` field is a plain `String` written by
    /// some other version of this console) wants to fall back to [`Theme::default`] silently,
    /// which is `unwrap_or_default()` at the call site. Answering with the default here would
    /// take the first of those away.
    ///
    /// The seam every selector lands on: [`select`] at startup, `console.theme` while the
    /// window is open, and [`Theme::resolve`] wherever a refusal has to be readable.
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "organon" => Some(Self::organon()),
            "light" => Some(Self::light()),
            "dark" => Some(Self::dark()),
            "chocolate" => Some(Self::chocolate()),
            _ => None,
        }
    }

    /// A palette **and the canonical name it answers to**, from [`Theme::NAMES`].
    ///
    /// The name matters as much as the palette wherever a choice is going to be *written
    /// down*: [`crate::prefs::Preferences::theme`] stores a `String`, and storing the caller's
    /// spelling rather than the table's would make the file a record of what someone typed
    /// instead of a record of which palette is selected. Today the two are identical because
    /// resolution is exact; that is precisely why binding them now costs nothing and why a
    /// later alias or case rule cannot quietly put an unresolvable name in the store.
    pub fn named(name: &str) -> Option<(Self, &'static str)> {
        let canonical = Self::NAMES.iter().find(|n| **n == name)?;
        Self::by_name(canonical).map(|t| (t, *canonical))
    }

    /// [`Theme::named`], with a refusal **carrying the list that would have worked**.
    ///
    /// 🚨 **Refused, never approximated** — `organon_core::kind::Kind::resolve`'s rule, for its
    /// reason. There is no nearest-match and no case folding: a palette silently substituted
    /// for the one asked for is indistinguishable from success, and the whole point of a theme
    /// verb is that you can tell which one you are looking at. Use this wherever somebody is
    /// reading the answer; use [`Theme::by_name`] where nobody is (`prefs.rs`'s
    /// `unwrap_or_default()` posture).
    pub fn resolve(name: &str) -> Result<(Self, &'static str), UnknownTheme> {
        Self::named(name).ok_or_else(|| UnknownTheme { word: name.to_string() })
    }

    /// egui's own chrome, derived from this palette.
    ///
    /// **Why this exists.** `console_main.rs` called `set_visuals(egui::Visuals::dark())` once, at
    /// construction, hardcoded — and that one call colours the sliders, the popup frames, the
    /// `TextEdit` selection wash and the scrollbars. A palette assembled only from `Theme`'s
    /// fields would leave those dark whatever it did, which is survivable for three dark
    /// palettes and fatal for a light one: `light` would read as broken rather than as light.
    ///
    /// **What it does not touch, on purpose.** Only colours are written. Corner radii, widget
    /// expansion, shadows and every other geometric field come from the egui base and are left
    /// exactly as they were — those are *form*, and form is a separate axis with a separate
    /// owner. `bg_stroke` and `fg_stroke` have their `color` assigned rather than being
    /// replaced, so each stroke keeps the width egui chose for it.
    ///
    /// 🚨 **[`Theme::organon`] returns `Visuals::dark()` unmodified**, so adding three palettes
    /// cannot restyle the console that already ships. That is the early return below, and
    /// `organon_chrome_is_still_egui_dark_to_the_byte` is what would catch it if someone
    /// "tidied" the special case away.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = match self.chrome {
            ChromeSource::EguiDark => return egui::Visuals::dark(),
            ChromeSource::DerivedDark => egui::Visuals::dark(),
            ChromeSource::DerivedLight => egui::Visuals::light(),
        };

        // Colour only — the width and the corner radius each widget already has are form.
        fn tint(
            w: &mut egui::style::WidgetVisuals,
            bg: Color32,
            weak: Color32,
            edge: Color32,
            fg: Color32,
        ) {
            w.bg_fill = bg;
            w.weak_bg_fill = weak;
            w.bg_stroke.color = edge;
            w.fg_stroke.color = fg;
        }

        v.dark_mode = self.chrome == ChromeSource::DerivedDark;

        // The five widget states map onto the console's own surface ladder: a widget nobody is
        // touching sits at the status band's weight, one under the pointer rises to the model
        // plate, and a pressed one rises again to that plate's own edge. `inactive`'s text is
        // the inactive tab's, which is the palette's existing answer to "present, not chosen".
        tint(&mut v.widgets.noninteractive, self.strip_fill, self.strip_fill, self.strip_edge, self.prose);
        tint(&mut v.widgets.inactive, self.model_fill, self.strip_fill, self.strip_edge, self.tab_inactive);
        tint(&mut v.widgets.hovered, self.model_fill, self.model_fill, self.model_edge, self.human_text);
        tint(&mut v.widgets.active, self.model_edge, self.model_edge, self.composer_edge_focus, self.human_text);
        tint(&mut v.widgets.open, self.model_fill, self.model_fill, self.model_edge, self.prose);

        // The selection wash is the user bubble's plate — the palette already had to decide
        // "the accent, tinted far enough into a surface that text stays readable on it", and
        // that is exactly the question a selection asks.
        v.selection.bg_fill = self.timeline_bubble_user;
        v.selection.stroke.color = self.prose;

        v.hyperlink_color = self.asking;
        v.faint_bg_color = self.strip_fill;
        // The `TextEdit` plate is the composer's, so egui's own text fields and the console's
        // hand-painted one cannot drift apart. `text_edit_bg_color` is left `None`, which means
        // "follow `extreme_bg_color`" — one answer, not two.
        v.extreme_bg_color = self.composer_fill;
        v.code_bg_color = self.surface_empty;
        v.warn_fg_color = self.mode_alert;
        v.error_fg_color = self.bad;

        // A popup is the harness menu's plate wearing the strongest border the palette has —
        // on a light page a window filled with the page colour has no other way to end.
        v.window_fill = self.tab_menu_fill;
        v.window_stroke.color = self.model_edge;
        v.panel_fill = self.term_bg;

        v
    }
}

/// The sixteen ANSI slots, named rather than numbered.
///
/// `ansi16` is one array field, so the field-list macro below cannot reach inside it — and an
/// editor showing `ansi16` as a single row would be an editor that cannot tune the terminal at
/// all. These are the names those sixteen positions answer to, in the order the array holds
/// them, which is the order every terminal in the world numbers them in.
pub const ANSI16_NAMES: [&str; 16] = [
    "ansi_black",
    "ansi_red",
    "ansi_green",
    "ansi_yellow",
    "ansi_blue",
    "ansi_magenta",
    "ansi_cyan",
    "ansi_white",
    "ansi_bright_black",
    "ansi_bright_red",
    "ansi_bright_green",
    "ansi_bright_yellow",
    "ansi_bright_blue",
    "ansi_bright_magenta",
    "ansi_bright_cyan",
    "ansi_bright_white",
];

/// 🚨 **The one place the palette's colour fields are enumerated.**
///
/// Everything that needs "every colour in a `Theme`" — the live editor, the override diff a
/// preference stores, the reader that applies one back — goes through [`Theme::fields`] and
/// [`Theme::fields_mut`], and both are generated from the single list below. A second
/// hand-written list is the failure this repo's one-vocabulary rule exists to prevent, and it
/// fails in the quietest possible way: a field added later simply stops being editable, with
/// nothing to say so.
///
/// ⚠️ **Rust has no reflection, so this list cannot be *derived* from the struct** — it is
/// hand-written, which means the real question is what catches it when the two disagree.
/// `every_colour_a_palette_can_differ_in_is_reachable` is that guard: it copies field-by-field
/// through this accessor between every ordered pair of the four palettes and asserts the
/// result is equal to the source. A field missing from the list keeps the destination's own
/// value and the comparison fails. ⚠️ Its residual blind spot, stated rather than left to be
/// discovered: a colour field on which **all four palettes agree to the byte** is invisible to
/// it. A fifth palette that disagrees anywhere closes that gap for that field.
///
/// The two non-colour fields — `scrim_floor` (a `u8`) and `chrome` (an enum) — are deliberately
/// **not** here. They are not pigment, an HSV editor has nothing to say about either, and the
/// coverage test copies them explicitly so that leaving them out cannot be mistaken for an
/// omission.
/// ⚠️ **The list is grouped, and the groups are the struct's own section headings.**
///
/// Sixty-eight colours will not fit in the band above the composer and are not navigable as one
/// flat run either — so the editor shows one group at a time, which is the same ring-narrowing
/// shape §1.9's command panel already uses for verbs. Taking the groups from *this* list rather
/// than from a second table beside it is the same rule as the fields themselves: a colour added
/// to a group is in the editor, and a colour added outside one does not compile.
macro_rules! colour_fields {
    ($($group:literal => [$($field:ident),+ $(,)?]),+ $(,)?) => {
        impl Theme {
            /// Every scalar colour field's name, in declaration order. `ansi16`'s sixteen are
            /// not here — they are [`ANSI16_NAMES`], appended by the accessors.
            pub const SCALAR_FIELDS: &'static [&'static str] = &[$($(stringify!($field)),+),+];

            /// The editor's rings: a heading, and the field names under it. The terminal's
            /// group carries [`ANSI16_NAMES`] on its tail, since those sixteen live in an
            /// array the macro cannot reach into but belong with the terminal in every other
            /// sense.
            pub const GROUPS: &'static [(&'static str, &'static [&'static str])] =
                &[$(($group, &[$(stringify!($field)),+])),+];

            /// Every colour this palette holds, by name, in declaration order.
            pub fn fields(&self) -> Vec<(&'static str, Color32)> {
                let mut out: Vec<(&'static str, Color32)> =
                    vec![$($((stringify!($field), self.$field)),+),+];
                out.extend(ANSI16_NAMES.iter().copied().zip(self.ansi16.iter().copied()));
                out
            }

            /// The same set, borrowed mutably — what an editor writes through.
            ///
            /// A `Vec` of borrows rather than an iterator because every element is a *distinct*
            /// field, so they are all live at once and the borrow checker is satisfied by the
            /// struct's own disjointness. A lending iterator would hand them out one at a time
            /// and buy nothing: the caller wants the list.
            pub fn fields_mut(&mut self) -> Vec<(&'static str, &mut Color32)> {
                let mut out: Vec<(&'static str, &mut Color32)> =
                    vec![$($((stringify!($field), &mut self.$field)),+),+];
                out.extend(ANSI16_NAMES.iter().copied().zip(self.ansi16.iter_mut()));
                out
            }
        }
    };
}

colour_fields! {
    "the transcript" => [human_text, human_fill, prose, dim],
    "cards" => [running, asking, ok, bad, surface_empty, card_left_rule],
    "the status strip" => [
        strip_fill,
        strip_edge,
        model_fill,
        model_edge,
        model_text,
        model_badge,
        mode_alert,
        mode_note,
        context_track,
        context_track_empty,
        context_arc,
        context_arc_high,
    ],
    "the composer" => [composer_fill, composer_edge, composer_edge_focus, composer_edge_dead],
    "the terminal" => [term_bg, term_fg, term_scrim_tint, term_exited_notice],
    "patch panels" => [panel_fill, panel_edge, panel_title, panel_text],
    "the timeline" => [
        timeline_scripted_fill,
        timeline_scripted_mark,
        timeline_status_pending,
        timeline_status_running,
        timeline_status_ok,
        timeline_status_failed,
        timeline_status_denied,
        timeline_status_cancelled,
        timeline_approval_accent,
        timeline_bubble_user,
        timeline_bubble_other,
    ],
    "the tab strip" => [
        tab_strip_fill,
        tab_active,
        tab_inactive,
        tab_plus,
        tab_menu_fill,
        tab_menu_installed,
        tab_menu_missing,
    ],
}

impl Theme {
    /// The editor's rings **with the sixteen ANSI slots folded into the terminal's**.
    ///
    /// [`Theme::GROUPS`] cannot carry them: they live inside the `ansi16` array, which the
    /// field macro has no way to name a member of. They are appended here rather than given a
    /// ring of their own because sixteen extra rows under "the terminal" is exactly where
    /// somebody tuning a terminal looks, and a group called `ansi` would be a tenth ring whose
    /// only distinction is an implementation detail of how the palette stores them.
    pub fn editor_groups() -> Vec<(&'static str, Vec<&'static str>)> {
        Self::GROUPS
            .iter()
            .map(|(group, fields)| {
                let mut fields: Vec<&'static str> = fields.to_vec();
                if *group == TERMINAL_GROUP {
                    fields.extend(ANSI16_NAMES);
                }
                (*group, fields)
            })
            .collect()
    }
}

/// The one group [`Theme::editor_groups`] appends [`ANSI16_NAMES`] to. Named rather than
/// spelled twice, so renaming the heading cannot silently orphan the sixteen.
pub const TERMINAL_GROUP: &str = "the terminal";

impl Theme {
    /// Write one colour by name. `false` for a name this palette does not have — **never a
    /// panic and never a near miss**, on [`Theme::resolve`]'s rule: an override read back out
    /// of a preferences file written by some other version of this console may name a field
    /// this build has since renamed, and the honest answer is to skip it and say so, not to
    /// guess which field was meant.
    pub fn set_field(&mut self, name: &str, colour: Color32) -> bool {
        for (field, slot) in self.fields_mut() {
            if field == name {
                *slot = colour;
                return true;
            }
        }
        false
    }

    /// Read one colour by name.
    pub fn field(&self, name: &str) -> Option<Color32> {
        self.fields().into_iter().find(|(f, _)| *f == name).map(|(_, c)| c)
    }

    /// Every colour in which this palette differs from `base`, by name.
    ///
    /// 🚨 **This is what gets stored, and storing the difference rather than the whole palette
    /// is the decision.** A saved override file holding all sixty-eight colours would pin the
    /// palette as it was on the day it was saved — so a later build that improves a shade
    /// nobody tuned would be silently overruled by a file the person thought recorded three
    /// edits. Storing only what was actually changed means an untouched field keeps following
    /// the compiled palette, which is what someone who tuned three colours expects to happen.
    pub fn diff(&self, base: &Theme) -> Vec<(&'static str, Color32)> {
        self.fields()
            .into_iter()
            .zip(base.fields())
            .filter(|((_, mine), (_, theirs))| mine != theirs)
            .map(|((name, mine), _)| (name, mine))
            .collect()
    }
}

/// One colour as eight hex digits, `rrggbbaa`, **exactly the bytes egui holds**.
///
/// 🚨 **Eight digits always, and the alpha is not decoration.** [`Theme::panel_fill`] is
/// premultiplied with an alpha of `0xe6` — the translucency *is* the look — so a six-digit
/// form would silently make every saved panel opaque. `Color32`'s bytes are already
/// premultiplied, so writing and reading them back through
/// [`Color32::from_rgba_premultiplied`] is byte-exact for every colour a palette can hold,
/// including the transparent [`Theme::card_left_rule`] two of the four palettes use to decline
/// a rule entirely.
pub fn to_hex(colour: Color32) -> String {
    let [r, g, b, a] = colour.to_array();
    format!("{r:02x}{g:02x}{b:02x}{a:02x}")
}

/// [`to_hex`]'s inverse. `None` for anything that is not eight hex digits, with or without a
/// leading `#` — a stored override is a string in a file some other build wrote, so the parse
/// has to be able to refuse rather than guess.
pub fn from_hex(text: &str) -> Option<Color32> {
    let text = text.strip_prefix('#').unwrap_or(text);
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&text[i..i + 2], 16).ok();
    Some(Color32::from_rgba_premultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?))
}

/// Lay a stored override map over a palette, returning a note for anything skipped.
///
/// ⚠️ **Every failure is a skip with a sentence, never a refusal of the whole file and never a
/// panic.** This map came out of `preferences.json`, which some other build of this console may
/// have written — so it can name a field this build has renamed, or carry a colour string that
/// is not eight hex digits. Dropping the whole palette back to its compiled state because one
/// line of it aged badly would lose the other nine edits; taking the nine and saying so about
/// the tenth is what a person tuning a palette actually wants. The notes go to the console's own
/// stderr beside the rest of [`select`]'s.
///
/// The returned notes are empty in the normal case, which is the case that matters: a file this
/// build wrote, read back by this build, says nothing at all.
pub fn apply_overrides(
    theme: &mut Theme,
    overrides: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let mut notes = Vec::new();
    for (field, text) in overrides {
        let Some(colour) = from_hex(text) else {
            notes.push(format!(
                "the stored colour for `{field}` is `{text}`, which is not eight hex digits — \
                 skipped"
            ));
            continue;
        };
        if !theme.set_field(field, colour) {
            notes.push(format!(
                "the stored palette names a colour `{field}`, which this console does not have \
                 — skipped"
            ));
        }
    }
    notes
}

/// The inverse: what a save writes down, given a palette and the compiled one it overrides.
///
/// Empty when nothing was tuned, and the caller is expected to *remove* the palette's entry
/// rather than store an empty map — see `Console::save_theme_overrides`.
pub fn collect_overrides(
    theme: &Theme,
    base: &Theme,
) -> std::collections::BTreeMap<String, String> {
    theme.diff(base).into_iter().map(|(name, c)| (name.to_string(), to_hex(c))).collect()
}

impl Default for Theme {
    fn default() -> Self {
        Self::organon()
    }
}

/// A name no palette answers to, carrying the names that do.
///
/// A type rather than a formatted `String`, on `organon_core::kind::UnknownKind`'s rule: the
/// startup resolution, the `console.theme` dispatch and the CLI all refuse an unknown name,
/// and three hand-written messages would eventually name three different sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTheme {
    /// Exactly what was asked for, unmodified — quoting it back is how a caller sees the
    /// stray quote or trailing space it did not know it had sent.
    pub word: String,
}

impl std::fmt::Display for UnknownTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not a theme — known themes: {}", self.word, Theme::NAMES.join(", "))
    }
}

/// The one-launch palette override.
///
/// # 🚨 Why `ORGANON_SHELL_THEME` and not `#38`'s `ORGANON_CONSOLE_THEME`
///
/// The issue that specified this tier spells it `ORGANON_CONSOLE_THEME`, and it is spelled
/// the other way here **deliberately**. The console reads six variables today —
/// `ORGANON_SHELL_BACKDROP`, `_SCRIM`, `_TABS`, `_DEFAULT`, `_CMD`, `_PTY_DEBUG` — and
/// `SHELL_ARCHITECTURE.md`'s header already records the naming gap as a *managed* one: the
/// artifact is `organon-console`, the crate, the feature, the IPC namespace and the variable
/// family are still `shell`, "because each is read by something else", and **issue #3 owns
/// closing the gap with deprecation aliases rather than find-and-replace**. Adding the first
/// `ORGANON_CONSOLE_*` variable would not close that gap; it would make the console read from
/// *two* prefixes at once, and #3's rename would then have to carry an exception for the one
/// variable that had already jumped. The trade being made is explicit: this file disagrees
/// with #38's literal text in order to agree with the six variables a person's launch shims
/// already set, and the rename gets one flag surface to move rather than one-and-a-bit.
pub const THEME_ENV: &str = "ORGANON_SHELL_THEME";

/// Where a resolved palette came from — the precedence order, as a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSource {
    /// [`THEME_ENV`], for this launch only. Never written back.
    Environment,
    /// `preferences.json` — what the person last chose with `organon console theme`.
    Stored,
    /// Nobody has ever said: [`Theme::default`].
    Default,
}

/// What [`select`] decided, and what the console should say about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// The palette to paint with.
    pub theme: Theme,
    /// Its canonical name, always one of [`Theme::NAMES`].
    pub name: &'static str,
    /// Which source won.
    pub source: ThemeSource,
    /// Lines the console prints on its own stderr, in order. **Empty is the normal case** —
    /// a stored palette that resolves and a fresh install both say nothing.
    pub notes: Vec<String>,
}

/// Resolve the palette this launch paints with: **environment, then stored preference, then
/// [`Theme::default`]**.
///
/// Pure — the caller reads the variable and the file and hands both in — so the precedence
/// order is testable without a process environment or a store on disk, which is the whole
/// reason it lives here rather than inline in `console_main.rs`.
///
/// # 📌 The environment DOES outrank the stored preference, amending `prefs.rs`'s decision
///
/// `prefs.rs` was written with the standing rule *"no environment variable overrides a stored
/// preference"*, and its reasoning is worth restating because it is still correct: a variable
/// baked into a launch shim wins **silently**, which is indistinguishable from the evaporation
/// the preferences file exists to end — and `organon-console.cmd` on this machine already
/// demonstrates the failure with `ORGANON_SHELL_TABS`. That decision was taken when *nothing*
/// could select a theme; the same paragraph names its own escape hatch, "a one-launch override
/// … belongs in a CLI flag that can say so in the console's own output".
///
/// The objection is to **silence**, not to precedence, and the console binary takes no flags —
/// it is launched by shims, and an environment variable *is* its argument surface. So the
/// override is granted and the objection is answered directly:
///
/// 1. **It announces itself, every launch**, naming the variable, the palette, and the stored
///    palette it is standing in front of ([`Selection::notes`]).
/// 2. **It never writes.** Only `organon console theme` stores anything, so an override cannot
///    destroy the choice underneath it — unset the variable and the stored palette is back.
///
/// Together those make it a *loan*, not a takeover: it can only win while it is set, and it
/// cannot win quietly. `SHELL_ARCHITECTURE.md` §1.5 carries the amendment.
///
/// # The failure posture
///
/// An unknown name at either level is **reported and skipped**, never fatal and never silently
/// defaulted: resolution falls through to the next source, so a typo in a shim leaves the
/// stored palette working rather than dropping the console back to `organon` as though nothing
/// had been chosen. An empty or whitespace-only variable is "unset" — that is the shim idiom
/// for clearing one, and treating it as a name would refuse a blank string at every launch.
pub fn select(env: Option<&str>, stored: Option<&str>) -> Selection {
    let mut notes = Vec::new();

    let env = env.map(str::trim).filter(|s| !s.is_empty());
    if let Some(word) = env {
        match Theme::resolve(word) {
            Ok((theme, name)) => {
                notes.push(match stored {
                    Some(had) if had != name => format!(
                        "theme `{name}` from {THEME_ENV}, overriding the stored `{had}` — this \
                         launch only, nothing is saved (`organon console theme <name>` is what \
                         stores a choice)"
                    ),
                    _ => format!(
                        "theme `{name}` from {THEME_ENV} — this launch only, nothing is saved"
                    ),
                });
                return Selection { theme, name, source: ThemeSource::Environment, notes };
            }
            Err(e) => notes.push(format!("{THEME_ENV}: {e} — ignored")),
        }
    }

    if let Some(word) = stored {
        match Theme::resolve(word) {
            Ok((theme, name)) => {
                return Selection { theme, name, source: ThemeSource::Stored, notes }
            }
            Err(e) => notes.push(format!(
                "the stored theme preference is unusable: {e} — using `{}`",
                DEFAULT_THEME_NAME
            )),
        }
    }

    Selection {
        theme: Theme::default(),
        name: DEFAULT_THEME_NAME,
        source: ThemeSource::Default,
        notes,
    }
}

/// The palette a console with nothing to go on paints with — [`Theme::NAMES`]'s first entry,
/// read from the table rather than restated, so "the default is listed first" stays a fact
/// about the list rather than a coincidence between two places.
pub const DEFAULT_THEME_NAME: &str = Theme::NAMES[0];

#[cfg(test)]
mod tests {
    use super::*;

    /// **The whole safety net for the extraction.** Every value below was read out of
    /// `main` — `git show main:native/organon-console/src/<file>` — *before* a line of it was
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

        // tabs.rs, and the strip's own plate from console_main.rs
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

    // -------------------------------------------------------------------------
    // The three palettes James specified
    // -------------------------------------------------------------------------

    /// 🚨 **Every hex the spec names, on the field the spec names it for.** These are not
    /// derived values and there is nothing to reason about: a spec said `#f7f8f9` is the panel,
    /// so `strip_fill` is `#f7f8f9` or the palette is not the one that was asked for. A wrong
    /// colour compiles and draws — this test is the only thing standing between "typed
    /// correctly" and "typed".
    ///
    /// ⚠️ **FOUR rows are deliberately no longer the spec's, and each is asserted *as* a
    /// deviation rather than quietly relaxed.** The spec's whole light surface ladder —
    /// `#ffffff` → `#f7f8f9` → `#e2e5e9` → `#c9ced6` — was moved down uniformly on James's
    /// instruction, twice, ending at `V ≈ 0.85`. The pins did not weaken: each still fails on
    /// any value nobody chose, they now just name the ladder constants instead of the spec's
    /// hexes. `the_light_page_stays_a_step_above_the_panel` below is what the *spec* used to
    /// provide for free by making the page pure white — nothing can be brighter than white, so
    /// the ordering could not be broken by editing the page. It can now.
    ///
    /// ⚠️ **The spec's TEXT and STATE rows are pinned to literals on purpose, and stayed
    /// literal.** They did not move, and pinning them to a constant that the surface roles also
    /// use would be the one edit that could let a future ladder correction drag the text ladder
    /// with it without any test noticing.
    #[test]
    fn the_specified_hexes_land_on_the_fields_they_were_specified_for() {
        let t = Theme::light();
        // page · panel/input · primary · secondary · faint · hairline · strong · accent ·
        // success · error
        assert_eq!(t.term_bg, Theme::LIGHT_PAGE, "light page — James's value, not the spec's");
        assert_eq!(Theme::LIGHT_PAGE, Color32::from_rgb(0xd7, 0xd8, 0xd9), "and it is #d7d8d9");
        // 🚨 The number he actually named: `V` in HSV is the largest channel over 255, and the
        // whole instruction was "about a 0.85 V". `217/255 = 0.8510` — the nearest a `u8` can
        // get to `0.85 × 255 = 216.75`. Asserted rather than left in a comment, because it is
        // the *request*, and a later edit that drifts the page is a later edit that stopped
        // answering it.
        let v = Theme::LIGHT_PAGE.b() as f32 / 255.0;
        assert!((v - 0.85).abs() < 0.005, "the light page is no longer at V ≈ 0.85: {v}");
        assert_eq!(t.term_scrim_tint, Theme::LIGHT_PAGE, "light scrim tint tracks the page");
        assert_eq!(t.composer_fill, Theme::LIGHT_PANEL, "light input plate");
        assert_eq!(t.strip_fill, Theme::LIGHT_PANEL, "light panel");
        assert_eq!(Theme::LIGHT_PANEL, Color32::from_rgb(0xd4, 0xd5, 0xd6), "and it is #d4d5d6");
        assert_eq!(t.prose, Color32::from_rgb(0x0f, 0x11, 0x14), "light primary text");
        assert_eq!(t.term_fg, Color32::from_rgb(0x0f, 0x11, 0x14), "light primary text");
        assert_eq!(t.model_badge, Color32::from_rgb(0x5d, 0x63, 0x6c), "light secondary");
        assert_eq!(t.dim, Color32::from_rgb(0x8b, 0x91, 0x9b), "light faint metadata");
        assert_eq!(t.strip_edge, Theme::LIGHT_HAIRLINE, "light hairline");
        assert_eq!(Theme::LIGHT_HAIRLINE, Color32::from_rgb(0xbf, 0xc2, 0xc6), "…it is #bfc2c6");
        assert_eq!(t.model_edge, Theme::LIGHT_STRONG, "light stronger border");
        assert_eq!(Theme::LIGHT_STRONG, Color32::from_rgb(0xa6, 0xab, 0xb3), "…it is #a6abb3");
        assert_eq!(t.asking, Color32::from_rgb(0x14, 0x40, 0xc4), "light accent blue");
        assert_eq!(t.ok, Color32::from_rgb(0x1a, 0x6b, 0x46), "light success");
        assert_eq!(t.bad, Color32::from_rgb(0xa3, 0x20, 0x20), "light error");
        // "No green anywhere" is about structure, so nothing that draws text, a border or a
        // surface may be green — but the named success colour stays green.
        for (name, c) in [
            ("prose", t.prose),
            ("term_fg", t.term_fg),
            ("dim", t.dim),
            ("term_bg", t.term_bg),
            ("strip_fill", t.strip_fill),
            ("strip_edge", t.strip_edge),
            ("model_edge", t.model_edge),
            ("tab_active", t.tab_active),
            ("panel_edge", t.panel_edge),
        ] {
            // Widened to `i32`. The page no longer overflows a `u8` at `+8` (`217 + 8` fits,
            // where `252 + 8` and `255 + 8` did not), but the widening stays: it is the page
            // that moves in this palette, and a bound that is only safe at today's value is a
            // bound that breaks on the next correction rather than on a real green cast.
            let (r, g, b) = (c.r() as i32, c.g() as i32, c.b() as i32);
            assert!(
                !(g > r + 8 && g > b + 8),
                "{name} is green-dominant in a palette that forbids structural green: {c:?}"
            );
        }

        let t = Theme::dark();
        assert_eq!(t.term_bg, Color32::from_rgb(0x0f, 0x11, 0x14), "dark page");
        assert_eq!(t.term_scrim_tint, Color32::from_rgb(0x0f, 0x11, 0x14), "dark scrim tint");
        assert_eq!(t.composer_fill, Color32::from_rgb(0x16, 0x18, 0x1c), "dark input plate");
        assert_eq!(t.strip_fill, Color32::from_rgb(0x16, 0x18, 0x1c), "dark panel");
        assert_eq!(t.prose, Color32::from_rgb(0xe6, 0xe9, 0xed), "dark primary");
        assert_eq!(t.term_fg, Color32::from_rgb(0xe6, 0xe9, 0xed), "dark primary");
        assert_eq!(t.model_badge, Color32::from_rgb(0x9a, 0xa1, 0xab), "dark secondary");
        assert_eq!(t.dim, Color32::from_rgb(0x6b, 0x72, 0x7c), "dark faint");
        assert_eq!(t.strip_edge, Color32::from_rgb(0x24, 0x27, 0x2d), "dark hairline");
        assert_eq!(t.model_edge, Color32::from_rgb(0x36, 0x3b, 0x43), "dark stronger border");
        assert_eq!(t.asking, Color32::from_rgb(0x8f, 0xb0, 0xff), "dark accent blue");
        assert_eq!(t.ok, Color32::from_rgb(0x6f, 0xbf, 0x95), "dark success");
        assert_eq!(t.bad, Color32::from_rgb(0xe8, 0x8b, 0x8b), "dark error");
        // "Never pure black" — the negative is calm, not switched off.
        assert_ne!(t.term_bg, Color32::BLACK);
        assert!(t.term_bg.r() >= 0x0f && t.term_bg.g() >= 0x0f && t.term_bg.b() >= 0x0f);

        let t = Theme::chocolate();
        assert_eq!(t.term_bg, Color32::from_rgb(0x19, 0x19, 0x19), "chocolate page");
        assert_eq!(t.term_scrim_tint, Color32::from_rgb(0x19, 0x19, 0x19), "chocolate scrim tint");
        assert_eq!(t.model_fill, Color32::from_rgb(0x26, 0x26, 0x26), "chocolate raised card");
        assert_eq!(t.human_fill, Color32::from_rgb(0x26, 0x26, 0x26), "chocolate raised card");
        assert_eq!(t.surface_empty, Color32::from_rgb(0x1f, 0x1f, 0x1f), "chocolate nested");
        assert_eq!(t.composer_fill, Color32::from_rgb(0x1f, 0x1f, 0x1f), "chocolate input");
        assert_eq!(t.tab_strip_fill, Color32::from_rgb(0x30, 0x30, 0x30), "chocolate tab surface");
        assert_eq!(t.tab_menu_fill, Color32::from_rgb(0x30, 0x30, 0x30), "chocolate raised");
        assert_eq!(t.strip_edge, Color32::from_rgb(0x3a, 0x3a, 0x3a), "chocolate hairline");
        assert_eq!(t.composer_edge, Color32::from_rgb(0x3a, 0x3a, 0x3a), "chocolate input border");
        assert_eq!(t.prose, Color32::from_rgb(0xed, 0xed, 0xed), "chocolate primary");
        assert_eq!(t.model_badge, Color32::from_rgb(0x8f, 0x8f, 0x8f), "chocolate secondary");
        assert_eq!(t.dim, Color32::from_rgb(0x6e, 0x6e, 0x6e), "chocolate muted");
        assert_eq!(t.timeline_status_ok, Color32::from_rgb(0x4c, 0xc7, 0x6e), "chocolate success");
        assert_eq!(t.asking, Color32::from_rgb(0x66, 0x99, 0xff), "chocolate accent blue");
        assert_eq!(t.bad, Color32::from_rgb(0xf0, 0x65, 0x5e), "chocolate error");
        // 🚨 The spec's own sentence, as an assertion: the WORD `ok` is the secondary grey.
        // `conversation_view` draws that word from `theme.ok`, so this is the field it names.
        assert_eq!(t.ok, Color32::from_rgb(0x8f, 0x8f, 0x8f), "the word `ok` is NOT green");
    }

    /// 🚨 **The light surface ladder still climbs, and the page is still the top of it.**
    ///
    /// This is the guard that became necessary the moment the page stopped being `#ffffff`.
    /// While it was pure white the ordering was free — nothing can be brighter than white, so
    /// "the page is above the panel" could not be broken by editing the page. It can now, and
    /// the failure is silent and *inverting*: a page darker than the panel makes every plate
    /// drawn on it — the composer, the status strip, a bubble — read as **raised out of** the
    /// paper rather than recessed into it, which is the opposite of the printed-publication
    /// metaphor the whole palette is built on. It compiles, it draws, and it looks like a
    /// theme rather than like a bug.
    ///
    /// ⚠️ **The trap is live, not hypothetical, and it has now been sprung twice.** The brief
    /// that commissioned the first correction suggested `#f4f5f6` as a starting point "which
    /// keeps a visible step above panel"; `0xf4` is **244** and the panel's `0xf7` was **247**,
    /// so that value sat three units *below* the panel and would have inverted the first step.
    /// The second correction — the page to `V ≈ 0.85` — is the same trap thirty units deeper:
    /// `217` against a panel of `249` inverts the ladder by **32**, which is why the whole
    /// ladder moved together rather than the page alone. The arithmetic is what settles it in
    /// both cases; the eye cannot judge three units of grey from a hex string.
    #[test]
    fn the_light_page_stays_a_step_above_the_panel() {
        let t = Theme::light();
        // The four steps, brightest first: page → panel → hairline → strong.
        let ladder = [
            ("page", t.term_bg),
            ("panel", t.strip_fill),
            ("hairline", t.strip_edge),
            ("strong", t.model_edge),
        ];
        for pair in ladder.windows(2) {
            let ((up, a), (down, b)) = (pair[0], pair[1]);
            for (ch, x, y) in [("r", a.r(), b.r()), ("g", a.g(), b.g()), ("b", a.b(), b.b())] {
                assert!(x > y, "the light ladder inverted at {up} → {down} on {ch}: {x} vs {y}");
            }
        }

        // The step itself, not merely its sign. Three units is what `LIGHT_PAGE` chose —
        // about half the 8/7/6 the spec's pure white had — and below roughly this the step
        // stops surviving a display's own gamma and the ladder has lost a distinction it was
        // built around, without any test noticing.
        let (page, panel) = (t.term_bg, t.strip_fill);
        for (ch, p, q) in
            [("r", page.r(), panel.r()), ("g", page.g(), panel.g()), ("b", page.b(), panel.b())]
        {
            assert!(p - q >= 3, "the page→panel step collapsed to {} on {ch}", p - q);
        }

        // 🚨 The stated derivation, as arithmetic rather than as a comment claiming it: the
        // page is *the panel plus a uniform offset*. Uniform is the whole point — it carries
        // the panel's own cool tilt (+1 green, +2 blue) up to the page instead of dropping a
        // neutral into a ladder where every other step is tilted. A page edited by eye would
        // pass every assertion above and quietly break this one.
        let step = page.r() - panel.r();
        assert_eq!(page.g() - panel.g(), step, "the page's offset from the panel is not uniform");
        assert_eq!(page.b() - panel.b(), step, "the page's offset from the panel is not uniform");

        // …and `panel_fill` is rule 4's function *of that page*, so it moved with it. `0xe6`
        // is `organon`'s alpha and does not move; the three channels are the page carrying it.
        assert_eq!(t.panel_fill.a(), 0xe6, "rule 4's alpha is `organon`'s own");
        for (ch, got, src) in [
            ("r", t.panel_fill.r(), page.r()),
            ("g", t.panel_fill.g(), page.g()),
            ("b", t.panel_fill.b(), page.b()),
        ] {
            let want = ((src as u32 * 0xe6 + 127) / 0xff) as u8;
            assert_eq!(got, want, "panel_fill.{ch} is not the page premultiplied at 0xe6");
        }
    }

    /// 🚨 **Every light plate that module rule 4 states as a *function* of a surface is
    /// recomputed from that surface — as arithmetic, not as a comment claiming it.**
    ///
    /// ⚠️ **This test exists because two of the three below had already gone stale, silently,
    /// for a day.** `composer_edge_dead` and `timeline_scripted_fill` are mixes "into the
    /// page"; both were computed against the spec's `#ffffff` and neither moved when the page
    /// became `#fafbfc` on 2026-08-14. `panel_fill` *did* move, and the reason is not that it
    /// was more important — it is that `the_light_page_stays_a_step_above_the_panel` pins it
    /// and nothing pinned these. A stated derivation with no test is a comment, and a comment
    /// cannot notice that it has stopped being true.
    ///
    /// ⚠️ The failure is not subtle once the page is a grey: a plate mixed into `#ffffff`
    /// sitting on a page at `V 0.851` **glows brighter than the page it interrupts**, which is
    /// precisely the glare the two corrections were removing. `timeline_bubble_user` is also
    /// `visuals().selection.bg_fill`, so it would be every text selection in the window.
    ///
    /// The rounding in each `want` is the rounding the comment beside each field states —
    /// nearest, half up — written as integer arithmetic so it is exact rather than a float
    /// that agrees today.
    #[test]
    fn every_light_plate_mixed_from_a_surface_is_recomputed_from_it() {
        let t = Theme::light();
        let (page, panel) = (t.term_bg, t.strip_fill);

        // "the error colour mixed 1:1 into the page"
        for (ch, got, e, p) in [
            ("r", t.composer_edge_dead.r(), t.bad.r(), page.r()),
            ("g", t.composer_edge_dead.g(), t.bad.g(), page.g()),
            ("b", t.composer_edge_dead.b(), t.bad.b(), page.b()),
        ] {
            let want = ((e as u32 + p as u32 + 1) / 2) as u8;
            assert_eq!(got, want, "composer_edge_dead.{ch} is not the error 1:1 into the page");
        }

        // "the error colour at 20 % over the page"
        for (ch, got, e, p) in [
            ("r", t.timeline_scripted_fill.r(), t.bad.r(), page.r()),
            ("g", t.timeline_scripted_fill.g(), t.bad.g(), page.g()),
            ("b", t.timeline_scripted_fill.b(), t.bad.b(), page.b()),
        ] {
            let want = ((2 * e as u32 + 8 * p as u32 + 5) / 10) as u8;
            assert_eq!(got, want, "timeline_scripted_fill.{ch} is not 20 % error over the page");
        }

        // "the accent at 1:5 over the panel"
        for (ch, got, a, p) in [
            ("r", t.timeline_bubble_user.r(), t.asking.r(), panel.r()),
            ("g", t.timeline_bubble_user.g(), t.asking.g(), panel.g()),
            ("b", t.timeline_bubble_user.b(), t.asking.b(), panel.b()),
        ] {
            let want = ((a as u32 + 5 * p as u32 + 3) / 6) as u8;
            assert_eq!(got, want, "timeline_bubble_user.{ch} is not accent 1:5 over the panel");
        }

        // …and the point of all three: a plate mixed into a surface can never end up brighter
        // than the surface it is mixed into, whatever the ladder does next. This is the
        // assertion that would have caught the two stale values without anybody re-deriving
        // them by hand.
        for (name, plate, base) in [
            ("composer_edge_dead", t.composer_edge_dead, page),
            ("timeline_scripted_fill", t.timeline_scripted_fill, page),
            ("timeline_bubble_user", t.timeline_bubble_user, panel),
        ] {
            for (ch, x, y) in [
                ("r", plate.r(), base.r()),
                ("g", plate.g(), base.g()),
                ("b", plate.b(), base.b()),
            ] {
                assert!(x <= y, "{name} is brighter than its own surface on {ch}: {x} vs {y}");
            }
        }
    }

    /// 🚨 **Chocolate's surface ladder is its spine, and every neutral in it is neutral.**
    /// "Warm graphite, never cool blue-grey" is a property that can be checked rather than
    /// admired: a blue cast is `b` running ahead of `r`, and the four named steps are the exact
    /// bytes the spec gives. A palette that drifted toward egui's own blue-tinted greys would
    /// still compile and would still look like a dark theme.
    #[test]
    fn chocolate_stays_neutral_and_keeps_its_ladder() {
        let t = Theme::chocolate();
        let ladder = [0x19u8, 0x1f, 0x26, 0x30];
        for (i, step) in ladder.iter().enumerate() {
            assert!(
                i == 0 || *step > ladder[i - 1],
                "the ladder must climb: {ladder:?} broke at step {i}"
            );
        }
        // Every surface and every neutral text weight: channels equal, so no blue cast.
        for (name, c) in [
            ("term_bg", t.term_bg),
            ("surface_empty", t.surface_empty),
            ("composer_fill", t.composer_fill),
            ("strip_fill", t.strip_fill),
            ("model_fill", t.model_fill),
            ("human_fill", t.human_fill),
            ("tab_strip_fill", t.tab_strip_fill),
            ("tab_menu_fill", t.tab_menu_fill),
            ("strip_edge", t.strip_edge),
            ("composer_edge", t.composer_edge),
            ("model_edge", t.model_edge),
            ("panel_edge", t.panel_edge),
            ("context_track", t.context_track),
            ("context_track_empty", t.context_track_empty),
            ("prose", t.prose),
            ("ok", t.ok),
            ("dim", t.dim),
            ("running", t.running),
            ("tab_inactive", t.tab_inactive),
        ] {
            assert!(
                c.r() == c.g() && c.g() == c.b(),
                "{name} is not a neutral: {c:?} — chocolate must never pick up a blue cast"
            );
        }
        // Blue is a label, never a wash: no surface may be the accent.
        for (name, c) in [
            ("term_bg", t.term_bg),
            ("strip_fill", t.strip_fill),
            ("model_fill", t.model_fill),
            ("tab_strip_fill", t.tab_strip_fill),
        ] {
            assert_ne!(c, t.asking, "{name} must not be an accent wash");
        }
    }

    /// **Every state a card can be in still reads as a different state.** The three new
    /// palettes drop `organon`'s amber (module rule 2 — no spec names one), so the pairs the
    /// field docs say must not read alike are exactly the pairs at risk from that decision.
    #[test]
    fn busy_blocked_done_and_failed_stay_four_different_answers() {
        for name in ["light", "dark", "chocolate"] {
            let t = Theme::by_name(name).expect("NAMES resolves");
            assert_ne!(t.running, t.asking, "{name}: 'busy' must not read as 'blocked on you'");
            assert_ne!(t.running, t.bad, "{name}: busy vs failed");
            assert_ne!(t.ok, t.bad, "{name}: done vs failed");
            assert_ne!(t.asking, t.bad, "{name}: blocked vs failed");
            // The field doc's standing rule: the permission alert is never the error colour,
            // because it sits on the band for hours and a permanent klaxon is skipped.
            assert_ne!(t.mode_alert, t.bad, "{name}: the mode alert must not be red");
            // The ring's two tracks, the contract `an_unmeasured_ring_is_distinguishable…`
            // states for `organon`: an unmeasured container is fainter than a measured one and
            // is still not the band it sits on.
            assert_ne!(t.context_track_empty, t.context_track, "{name}: the ring's two tracks");
            assert_ne!(t.context_track_empty, t.strip_fill, "{name}: an invisible track");
        }
    }

    /// Names resolve, round-trip, and a name this build does not have is a `None` rather than a
    /// panic or a silent substitution.
    #[test]
    fn every_name_resolves_and_an_unknown_one_falls_back() {
        assert_eq!(Theme::NAMES.len(), 4);
        for name in Theme::NAMES {
            assert!(Theme::by_name(name).is_some(), "{name} is listed but does not resolve");
        }
        assert_eq!(Theme::by_name("organon"), Some(Theme::organon()));
        assert_eq!(Theme::by_name("light"), Some(Theme::light()));
        assert_eq!(Theme::by_name("dark"), Some(Theme::dark()));
        assert_eq!(Theme::by_name("chocolate"), Some(Theme::chocolate()));
        // The four are genuinely four.
        assert_ne!(Theme::light(), Theme::dark());
        assert_ne!(Theme::dark(), Theme::chocolate());
        assert_ne!(Theme::chocolate(), Theme::organon());
        // A miss is a miss — including the near-misses a stored file or a typed word produces.
        for miss in ["", "Organon", "ORGANON", " organon", "light ", "solarized", "choc"] {
            assert_eq!(Theme::by_name(miss), None, "{miss:?} must not resolve");
        }
        // …and the documented way a caller turns that into a palette is the default, which is
        // still `organon`. This is the line `prefs.rs`'s stored name will be read through.
        assert_eq!(Theme::by_name("nonesuch").unwrap_or_default(), Theme::organon());
    }

    /// 🚨 **Adding three palettes must not restyle the console that ships.** `console_main.rs`
    /// called `set_visuals(egui::Visuals::dark())` once, hardcoded; it now calls
    /// `theme.visuals()`, and for `organon` the two have to be the same value to the byte —
    /// otherwise every slider, popup frame, selection wash and scrollbar in the current console
    /// silently changed while nobody was looking at a new palette.
    #[test]
    fn organon_chrome_is_still_egui_dark_to_the_byte() {
        assert_eq!(Theme::organon().visuals(), egui::Visuals::dark());
        assert_eq!(Theme::organon().chrome, ChromeSource::EguiDark);
    }

    /// **The derivation actually runs, and it moves the surfaces a hardcoded dark chrome would
    /// have left behind.** The failure this guards is the quiet one: a `visuals()` that returns
    /// its base unchanged would pass the `organon` pin above and leave `light` sitting in
    /// egui's dark widget chrome — which is exactly the "half the pixels stay dark" failure the
    /// method exists to fix.
    #[test]
    fn a_derived_chrome_carries_the_palette_rather_than_eguis() {
        for name in ["light", "dark", "chocolate"] {
            let t = Theme::by_name(name).expect("NAMES resolves");
            let v = t.visuals();
            assert_ne!(v, egui::Visuals::dark(), "{name}: chrome never left egui's dark base");
            assert_ne!(v, egui::Visuals::light(), "{name}: chrome never left egui's light base");
            // The four surfaces that decide whether a window reads as this palette at all.
            assert_eq!(v.panel_fill, t.term_bg, "{name}: the panel is the page");
            assert_eq!(v.extreme_bg_color, t.composer_fill, "{name}: a TextEdit is the composer");
            assert_eq!(v.window_fill, t.tab_menu_fill, "{name}: a popup is the menu's plate");
            assert_eq!(v.selection.bg_fill, t.timeline_bubble_user, "{name}: the selection wash");
            assert_eq!(v.error_fg_color, t.bad, "{name}");
            assert_eq!(v.warn_fg_color, t.mode_alert, "{name}");
            assert_eq!(v.hyperlink_color, t.asking, "{name}");
            // `text_edit_bg_color` is left `None` on purpose: it means "follow
            // extreme_bg_color", so the composer's plate is stated once rather than twice.
            assert_eq!(v.text_edit_bg_color, None, "{name}");
        }
        // `dark_mode` is egui's own summary of the palette's polarity, and a light palette that
        // reported itself dark would make every egui default that consults it wrong.
        assert!(!Theme::light().visuals().dark_mode, "light is not dark mode");
        assert!(Theme::dark().visuals().dark_mode);
        assert!(Theme::chocolate().visuals().dark_mode);
    }

    /// ⚠️ **The chrome derivation writes colours and nothing else.** Corner radii, widget
    /// expansion, stroke *widths* and shadows are form — a separate axis with a separate owner
    /// — so every one of them must still be whatever the egui base said. This is the test that
    /// notices if a colour change ever grows a geometry change beside it.
    #[test]
    fn the_chrome_derivation_never_touches_form() {
        let bases = [
            (Theme::light().visuals(), egui::Visuals::light()),
            (Theme::dark().visuals(), egui::Visuals::dark()),
            (Theme::chocolate().visuals(), egui::Visuals::dark()),
        ];
        for (derived, base) in bases {
            assert_eq!(derived.window_corner_radius, base.window_corner_radius);
            assert_eq!(derived.menu_corner_radius, base.menu_corner_radius);
            assert_eq!(derived.window_shadow, base.window_shadow);
            assert_eq!(derived.popup_shadow, base.popup_shadow);
            assert_eq!(derived.window_stroke.width, base.window_stroke.width);
            for (d, b) in [
                (&derived.widgets.noninteractive, &base.widgets.noninteractive),
                (&derived.widgets.inactive, &base.widgets.inactive),
                (&derived.widgets.hovered, &base.widgets.hovered),
                (&derived.widgets.active, &base.widgets.active),
                (&derived.widgets.open, &base.widgets.open),
            ] {
                assert_eq!(d.corner_radius, b.corner_radius, "a corner radius moved");
                assert_eq!(d.expansion, b.expansion, "a widget expansion moved");
                assert_eq!(d.bg_stroke.width, b.bg_stroke.width, "a bg stroke width moved");
                assert_eq!(d.fg_stroke.width, b.fg_stroke.width, "a fg stroke width moved");
            }
        }
    }

    /// **`organon` is byte-for-byte what it was, including the two new non-pigment fields.**
    /// The pin above covers every colour; this covers the pair added for the palettes, because
    /// a scrim floor that moved would change what the shipped console paints over its backdrop
    /// without changing a single colour value.
    #[test]
    fn organon_is_unchanged_by_the_new_palettes() {
        let t = Theme::organon();
        assert_eq!(t.scrim_floor, crate::term_view::SCRIM_FLOOR, "PRD §4.6's own number");
        assert_eq!(t.scrim_floor, 96, "…and it is still literally 96");
        assert_eq!(t.chrome, ChromeSource::EguiDark);
        assert_eq!(Theme::default(), t);
    }

    // ── Selection: who decides which palette a launch paints with ──────────────

    /// CONTRACT: **the precedence order**, stated once as a test rather than as prose.
    /// Environment beats the stored preference beats the default, and each rung is exercised
    /// with the ones above it absent.
    #[test]
    fn the_environment_beats_the_store_beats_the_default() {
        let env_wins = select(Some("light"), Some("dark"));
        assert_eq!(env_wins.name, "light");
        assert_eq!(env_wins.source, ThemeSource::Environment);
        assert_eq!(env_wins.theme, Theme::light());

        let stored_wins = select(None, Some("dark"));
        assert_eq!(stored_wins.name, "dark");
        assert_eq!(stored_wins.source, ThemeSource::Stored);
        assert_eq!(stored_wins.theme, Theme::dark());

        let nothing = select(None, None);
        assert_eq!(nothing.name, DEFAULT_THEME_NAME);
        assert_eq!(nothing.source, ThemeSource::Default);
        assert_eq!(nothing.theme, Theme::organon());
    }

    /// CONTRACT: **`organon` is still the answer when nothing is set** — including for the two
    /// shapes of "set to nothing" a launch shim actually produces. `set VAR=` on Windows and
    /// `VAR=` in a POSIX shell differ, and neither is a request for a palette named "".
    #[test]
    fn organon_remains_the_answer_when_nothing_is_chosen() {
        for env in [None, Some(""), Some("   "), Some("\t")] {
            let s = select(env, None);
            assert_eq!(s.name, "organon", "{env:?}");
            assert_eq!(s.theme, Theme::organon(), "{env:?}");
            assert_eq!(s.source, ThemeSource::Default, "{env:?}");
            assert!(s.notes.is_empty(), "{env:?}: an unset variable says nothing");
        }
        // …and an empty variable does not blank out a stored choice either.
        assert_eq!(select(Some(""), Some("dark")).name, "dark");
    }

    /// CONTRACT: **every palette in the table is reachable by name**, from either source.
    /// This is the test that fails if a fifth palette is added without a `by_name` arm — the
    /// exact shape of "the picker offers a name nothing can paint".
    #[test]
    fn every_name_is_selectable_from_either_source() {
        for name in Theme::NAMES {
            let by_env = select(Some(name), None);
            assert_eq!(by_env.name, name);
            assert_eq!(by_env.theme, Theme::by_name(name).expect("NAMES resolves"));
            assert_eq!(by_env.source, ThemeSource::Environment);

            let by_store = select(None, Some(name));
            assert_eq!(by_store.name, name);
            assert_eq!(by_store.theme, Theme::by_name(name).expect("NAMES resolves"));
            assert_eq!(by_store.source, ThemeSource::Stored);
            assert!(by_store.notes.is_empty(), "{name}: a stored palette that works is silent");
        }
    }

    /// CONTRACT: **an unknown name falls through, and the complaint names the known set.**
    /// Both halves matter. Falling through rather than resetting is what keeps a typo in a
    /// launch shim from silently discarding a stored choice; naming the set is what makes the
    /// message actionable, and it is quoted from [`Theme::NAMES`] so it cannot offer a palette
    /// this build has no way to paint.
    #[test]
    fn an_unknown_name_falls_through_and_names_the_known_set() {
        let s = select(Some("phosphor"), Some("dark"));
        assert_eq!(s.name, "dark", "the stored choice survives a bad override");
        assert_eq!(s.source, ThemeSource::Stored);
        let note = s.notes.join("\n");
        assert!(note.contains(THEME_ENV), "{note:?} must name the variable at fault");
        assert!(note.contains("phosphor"), "{note:?} must quote what was asked for");
        for name in Theme::NAMES {
            assert!(note.contains(name), "{note:?} must offer `{name}`");
        }

        // Both unusable ⇒ the default, and both complaints are reported rather than the
        // first one swallowing the second.
        let s = select(Some("Light"), Some("nonsense"));
        assert_eq!(s.name, DEFAULT_THEME_NAME, "case is not folded — resolution is exact");
        assert_eq!(s.source, ThemeSource::Default);
        assert_eq!(s.notes.len(), 2, "{:?}", s.notes);

        // A stored name a *newer* console wrote is the same case, and must not be fatal.
        let s = select(None, Some("solarized"));
        assert_eq!(s.name, DEFAULT_THEME_NAME);
        assert!(s.notes[0].contains("solarized"), "{:?}", s.notes);
    }

    /// CONTRACT: **an override says so, every launch, and says what it is standing in front
    /// of.** This is the whole amendment to `prefs.rs`'s "no variable overrides a stored
    /// preference" — the objection there is to a shim winning *silently*, and this is the line
    /// that answers it. A message that did not name the stored palette would leave a person
    /// unable to tell an override from a lost preference, which is the failure being avoided.
    #[test]
    fn an_override_announces_itself_and_names_what_it_shadows() {
        let s = select(Some("chocolate"), Some("light"));
        assert_eq!(s.source, ThemeSource::Environment);
        assert_eq!(s.notes.len(), 1);
        let note = &s.notes[0];
        assert!(note.contains(THEME_ENV), "{note:?}");
        assert!(note.contains("chocolate") && note.contains("light"), "{note:?}");
        assert!(note.contains("nothing is saved"), "{note:?}: the loan has to be visible");

        // Overriding with the *same* palette that is stored is not shadowing anything, so the
        // line does not claim it is. It still announces, because the variable is still what
        // decided this launch.
        let same = select(Some("light"), Some("light"));
        assert_eq!(same.source, ThemeSource::Environment);
        assert_eq!(same.notes.len(), 1);
        assert!(!same.notes[0].contains("overriding"), "{:?}", same.notes);
    }

    /// CONTRACT: the canonical name travels with the palette, because it is what gets
    /// **written down**. A selection that resolved must always report a name the store can
    /// read back — i.e. one of [`Theme::NAMES`], never the caller's spelling.
    #[test]
    fn a_selection_always_reports_a_storable_name() {
        for (env, stored) in [
            (Some("light"), None),
            (None, Some("chocolate")),
            (Some("nope"), Some("dark")),
            (Some("nope"), Some("nope")),
            (None, None),
        ] {
            let s = select(env, stored);
            assert!(Theme::NAMES.contains(&s.name), "{s:?}");
            assert_eq!(Theme::by_name(s.name).as_ref(), Some(&s.theme), "{s:?}");
        }
    }

    /// CONTRACT: the refusal type is the one message everybody prints. Pinned so the three
    /// call sites cannot drift into three different sentences.
    #[test]
    fn an_unknown_theme_prints_the_table() {
        let e = Theme::resolve("phosphor").unwrap_err();
        assert_eq!(e.word, "phosphor");
        assert_eq!(
            e.to_string(),
            "`phosphor` is not a theme — known themes: organon, light, dark, chocolate"
        );
        assert_eq!(Theme::resolve("organon").unwrap().1, "organon");
    }

    /// CONTRACT: **the default name is read off the table**, not restated. If someone reorders
    /// `NAMES` this test is the one that notices the default moved with it.
    #[test]
    fn the_default_name_is_the_first_row_of_the_table() {
        assert_eq!(DEFAULT_THEME_NAME, Theme::NAMES[0]);
        assert_eq!(Theme::by_name(DEFAULT_THEME_NAME), Some(Theme::default()));
    }

    /// 🚨 **CONTRACT: the field accessor reaches every colour a palette can differ in.**
    ///
    /// This is the whole guard on the hand-written list in `colour_fields!`, and the reason it
    /// is written this way rather than as a count. A count (`assert_eq!(fields().len(), 68)`)
    /// moves the hardcoding rather than removing it: someone adding a field updates the number
    /// and the test goes green while the accessor still cannot see it.
    ///
    /// Instead: copy **through the accessor** from one palette into another, add the two
    /// non-colour fields by hand, and demand the result equal the source. A colour field the
    /// accessor cannot reach keeps the destination's own value, and `assert_eq!` fails naming
    /// the pair. Every ordered pair is checked, so a field is covered as soon as *any* two of
    /// the four palettes disagree about it.
    ///
    /// ⚠️ Stated rather than left to be found: a colour on which **all four palettes agree to
    /// the byte** is invisible here. Nothing else in the suite would catch it either; a fifth
    /// palette that differs anywhere closes the gap for that field.
    #[test]
    fn every_colour_a_palette_can_differ_in_is_reachable() {
        let palettes: Vec<(&str, Theme)> =
            Theme::NAMES.iter().map(|n| (*n, Theme::by_name(n).unwrap())).collect();

        for (from_name, from) in &palettes {
            for (into_name, into) in &palettes {
                let mut copy = into.clone();
                for (name, slot) in copy.fields_mut() {
                    // `fields` is in the same declaration order for every palette, so this
                    // could be a zip — the lookup by name is deliberate, because it is also
                    // testing that the two directions agree about the names.
                    *slot = from
                        .field(name)
                        .unwrap_or_else(|| panic!("`{name}` is writable but not readable"));
                }
                // The two fields that are not pigment, by hand — see `colour_fields!`.
                copy.scrim_floor = from.scrim_floor;
                copy.chrome = from.chrome;
                assert_eq!(
                    &copy, from,
                    "copying `{from_name}` into `{into_name}` through `fields_mut` left them \
                     different, so `colour_fields!` is missing a field"
                );
            }
        }
    }

    /// CONTRACT: a name appears once. A duplicate would make `set_field` write the first of
    /// two fields forever and the second one uneditable, which is the accessor failing in a
    /// way the coverage test above cannot see (it looks up by name and would find the first).
    #[test]
    fn no_two_colours_answer_to_the_same_name() {
        let mut seen = std::collections::BTreeSet::new();
        for (name, _) in Theme::organon().fields() {
            assert!(seen.insert(name), "`{name}` is the name of two different colours");
        }
        assert_eq!(seen.len(), Theme::SCALAR_FIELDS.len() + ANSI16_NAMES.len());
    }

    /// CONTRACT: hex is **byte-exact both ways, alpha included**. `panel_fill` is the case that
    /// matters — it is premultiplied at `0xe6`, so a six-digit round trip would quietly make
    /// every saved panel opaque — and `card_left_rule` is the other, being fully transparent in
    /// two of the four palettes.
    #[test]
    fn every_colour_survives_the_hex_round_trip() {
        for name in Theme::NAMES {
            let t = Theme::by_name(name).unwrap();
            for (field, colour) in t.fields() {
                let text = to_hex(colour);
                assert_eq!(text.len(), 8, "{name}.{field}");
                assert_eq!(from_hex(&text), Some(colour), "{name}.{field} via `{text}`");
                assert_eq!(from_hex(&format!("#{text}")), Some(colour), "{name}.{field} with #");
            }
        }
        assert_eq!(to_hex(Theme::organon().panel_fill), "0b120ee6", "the alpha is carried");
        assert_eq!(to_hex(Color32::TRANSPARENT), "00000000");
    }

    /// CONTRACT: a stored override that is not eight hex digits is **refused**, not guessed at.
    #[test]
    fn a_malformed_hex_override_is_refused() {
        for bad in ["", "#", "c8e6c8", "c8e6c8ff00", "gggggggg", "#c8e6c8f", " c8e6c8ff"] {
            assert_eq!(from_hex(bad), None, "`{bad}` should not parse");
        }
    }

    /// CONTRACT: `diff` reports exactly what was changed, so a saved override file records
    /// three edits rather than a whole frozen palette. See [`Theme::diff`] for why that
    /// matters.
    #[test]
    fn a_diff_names_only_what_moved() {
        let base = Theme::light();
        assert!(base.diff(&base).is_empty(), "a palette differs from itself in nothing");

        let mut edited = base.clone();
        assert!(edited.set_field("term_bg", Color32::from_rgb(0xf2, 0xf2, 0xf2)));
        assert!(edited.set_field("ansi_bright_white", Color32::from_rgb(0x10, 0x10, 0x10)));
        let moved = edited.diff(&base);
        assert_eq!(moved.len(), 2, "{moved:?}");
        assert_eq!(moved[0].0, "term_bg", "declaration order, not alphabetical");
        assert_eq!(moved[1].0, "ansi_bright_white");

        // …and applying the diff back onto the base reconstructs the edited palette, which is
        // the whole contract between `diff` and what a launch reads out of the store.
        let mut rebuilt = base.clone();
        for (name, colour) in moved {
            assert!(rebuilt.set_field(name, colour));
        }
        assert_eq!(rebuilt, edited);
    }

    /// 🚨 **CONTRACT: the editor's rings are the whole field set, partitioned — nothing missing
    /// and nothing shown twice.** This is what stops a colour existing in the palette, being
    /// reachable through `set_field`, and yet appearing on no page of the editor: reachable but
    /// unreachable-by-hand, which from a person's side is indistinguishable from not existing.
    #[test]
    fn the_editor_rings_partition_every_colour() {
        let grouped: Vec<&str> =
            Theme::editor_groups().into_iter().flat_map(|(_, f)| f).collect();
        let all: Vec<&str> = Theme::organon().fields().into_iter().map(|(n, _)| n).collect();

        let mut sorted_grouped = grouped.clone();
        sorted_grouped.sort_unstable();
        let mut sorted_all = all.clone();
        sorted_all.sort_unstable();
        assert_eq!(sorted_grouped, sorted_all, "the rings and the field set disagree");
        assert_eq!(grouped.len(), all.len(), "a field appears on two rings");

        // The heading the sixteen hang off has to keep existing, or they orphan silently.
        assert!(
            Theme::GROUPS.iter().any(|(g, _)| *g == TERMINAL_GROUP),
            "`{TERMINAL_GROUP}` is not one of the groups, so ANSI16_NAMES attach to nothing"
        );
        let terminal = Theme::editor_groups()
            .into_iter()
            .find(|(g, _)| *g == TERMINAL_GROUP)
            .expect("the terminal ring")
            .1;
        assert!(terminal.contains(&"ansi_bright_white"), "{terminal:?}");
        assert!(terminal.contains(&"term_bg"), "{terminal:?}");
    }

    /// CONTRACT: a stored override survives the round trip through the file's string form, and
    /// **only the tuned colours are written**. This is the contract between a save and the next
    /// launch; if it breaks, a palette silently opens as something other than what was saved.
    #[test]
    fn overrides_round_trip_through_the_store() {
        let base = Theme::light();
        let mut tuned = base.clone();
        tuned.set_field("term_bg", Color32::from_rgb(0xf0, 0xf0, 0xf0));
        tuned.set_field("panel_fill", Color32::from_rgba_premultiplied(0xd0, 0xd0, 0xd0, 0xc0));

        let stored = collect_overrides(&tuned, &base);
        assert_eq!(stored.len(), 2, "only what moved is written: {stored:?}");
        assert_eq!(stored["term_bg"], "f0f0f0ff");
        assert_eq!(stored["panel_fill"], "d0d0d0c0", "the alpha is stored");

        let mut rebuilt = Theme::light();
        assert!(apply_overrides(&mut rebuilt, &stored).is_empty(), "a clean file says nothing");
        assert_eq!(rebuilt, tuned);
    }

    /// 🚨 CONTRACT: **a stale or malformed override is skipped with a sentence, and the rest of
    /// the file still lands.** The file may have been written by a build with a field this one
    /// has renamed; losing nine good edits over the tenth would be the wrong trade.
    #[test]
    fn a_stale_override_is_skipped_and_the_others_still_apply() {
        let mut t = Theme::light();
        let stored = std::collections::BTreeMap::from([
            ("term_bg".to_string(), "f0f0f0ff".to_string()),
            ("human_txt".to_string(), "112233ff".to_string()),
            ("prose".to_string(), "not-a-colour".to_string()),
        ]);
        let notes = apply_overrides(&mut t, &stored);

        assert_eq!(t.term_bg, Color32::from_rgb(0xf0, 0xf0, 0xf0), "the good one did not land");
        assert_eq!(t.prose, Theme::light().prose, "a malformed colour must change nothing");
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("`human_txt`") && n.contains("does not have")));
        assert!(notes.iter().any(|n| n.contains("`prose`") && n.contains("eight hex digits")));
    }

    /// CONTRACT: an unknown field name is refused rather than silently dropped on the floor or
    /// matched to something near it — a file written by another build is the case.
    #[test]
    fn an_unknown_field_name_is_refused() {
        let mut t = Theme::organon();
        assert!(!t.set_field("human_txt", Color32::RED), "no near-miss matching");
        assert!(!t.set_field("scrim_floor", Color32::RED), "not a colour, not in the list");
        assert!(!t.set_field("ansi16", Color32::RED), "the array is reached by slot, not whole");
        assert_eq!(t, Theme::organon(), "a refused write changes nothing");
        assert!(t.set_field("ansi_black", Color32::RED), "the slots themselves are reachable");
    }
}

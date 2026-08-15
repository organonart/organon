//! **Screen — how much of the display the window covers.**
//!
//! [`crate::theme`] answers *what the console is made of*. [`crate::posture`] answers *how it
//! stands* — tight and square like a terminal, or open and inset like a desktop document.
//! This answers a third question that is neither: **how much of the display the window
//! takes.** Windowed, or filling it edge to edge with no title bar.
//!
//! # 🚨 This was asked for as "a new posture", and it cannot be one
//!
//! James's words were *"adds a new posture, which is full screen"*, and the obvious reading —
//! a third slot beside `terminal` and `desktop` — is **not available**, for a reason
//! [`crate::posture`] states about itself: [`crate::posture::Posture`] is a **scalar**, not an
//! enum. `Form::at(t)` lerps componentwise between two ends and `Posture::from_scalar` takes
//! anything between them, so `organon console posture 0.5` is a real drawable console. There
//! is no third slot to add, because there are no slots — there is an axis.
//!
//! And full screen is not a *point* on that axis. Every one of `Form`'s fourteen tokens is a
//! margin, a corner, a padding, a line height, a gap, a tracking, or the presence of a border,
//! a rule or a tick. **Full screen changes none of them.** It changes the rectangle the window
//! occupies, which no token in that struct describes.
//!
//! ## It passes the module's own orthogonality test, verbatim
//!
//! [`crate::posture`] opens by arguing that theme and posture are *"orthogonal on purpose —
//! `organon` at desktop posture and a light palette at terminal posture are both real things,
//! and neither is a variant of the other."* Apply the same test here and it answers the same
//! way: a **full-screen terminal** is the oldest thing in computing, and a **full-screen
//! desktop document** is what every reader app opens into. Both are real; neither is a variant
//! of the other. So this is a third orthogonal state, and every one of the (posture × screen)
//! combinations is a console somebody would want.
//!
//! # ⚠️ Why the form is NOT nudged when the window fills the display
//!
//! The tempting version — full screen also opens the margins out, because a 2560-wide window
//! wants more air than an 1100-wide one — couples to the **wrong variable**, and that is worth
//! stating precisely because the argument *for* it is a good one.
//!
//! A **maximized** 2560-wide window and a **full-screen** 2560-wide window are the same width
//! and want the same margins. A full-screen window on a 1280-wide laptop is *narrower* than a
//! maximized one on this workstation's display, and wants today's desktop margins unchanged.
//! So "is it full screen" is not the question the margin wanted asked — "how wide is it" is.
//! If a width-responsive form is ever wanted, its input is `available_width`, and it is a
//! change to how a `Form` is resolved rather than a third state feeding into it.
//!
//! The coupling would also be the part that is regretted: with it, `organon console posture
//! terminal` typed while full screen either draws something that is not the terminal posture,
//! or is refused. A person who typed a posture would not get the posture they typed. Keeping
//! the two apart means the two verbs mean exactly what they say, at all times, in every
//! combination.
//!
//! # ⚠️ No state is held here, and that is deliberate
//!
//! There is no `Screen` field on `Console`. The window itself knows whether it is full screen
//! (`winit::window::Window::fullscreen` answers), so [`Screen`] is derived from the window at
//! the moment a command arrives rather than remembered beside it. A remembered copy is a
//! second source of truth for one boolean, and this machine's own change log is a catalogue of
//! what those cost: a status line that cannot disagree with reality is not a status line. The
//! concrete failure it forecloses is a window put full screen by the platform rather than by
//! this verb — macOS's green button, a tiling window manager — after which a remembered
//! `Windowed` would make [`ScreenCmd::Toggle`] send the window *into* full screen from full
//! screen, and the one command whose whole job is "the other one" would do nothing visible.
//!
//! ⚠️ `bin/visual.rs`'s `sync_fullscreen` *does* keep such a bool, and it is right to: its
//! intent arrives from `World::wants_fullscreen` on every frame, so it needs an edge to act on.
//! Nothing here arrives periodically, so there is no edge to detect and no bool to keep.
//!
//! # 🚨 The way out is a key, and choosing it needed an argument
//!
//! A window filling a display with no title bar and no way back is a trap. The verb is
//! reachable — the console is full of terminals and `organon console screen windowed` works
//! from any of them — but that is a way out for somebody who remembers the verb.
//!
//! **Escape is not available**, and `CONSOLE_ARCHITECTURE.md` §1.2 owns why: in a terminal tab
//! the keyboard is the child's, `vim` needs Escape, and taking it would have to be conditional
//! on state — which is unbuilt work with a designed mechanism reserved for it (consuming out
//! of `i.events` before `term_view` clones them). Spending that here would be borrowing a
//! whole subsystem to pay for one window flag.
//!
//! **[`CHORD`] is free, and that is measured rather than assumed.** [`crate::term::encode_key`]
//! returns `None` for every function key, so the console has never sent it to a child under any
//! modifiers; [`crate::conversation_view::palette_key`] and [`crate::theme_edit::edit_key`]
//! both answer their `Ignore` for it. Claiming it takes nothing from anybody, which is exactly
//! what Escape could not say — and it is the convention every full-screen window on this
//! platform already uses, so it is the key a hand reaches for without being told. All three
//! facts are pinned by tests below, so the day one of them stops being true, this says so.
//!
//! The claim is therefore **unconditional** — every tab, every state, whatever has focus — and
//! needs none of the state-dependent machinery Escape would. It is read at the same site as
//! `⌘T`/`⌘W`/`⌘1-9` (`console_main.rs`'s `redraw`, from the raw frame events before any panel
//! is laid out), which is what makes it work while the composer has focus.

/// Whether the console's window fills the display.
///
/// **Derived from the window, never stored** — see this module's header for why. It exists as
/// a type rather than a `bool` so [`ScreenCmd::apply_to`] reads as what it is, and so a
/// refusal can name the two states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Screen {
    /// An ordinary window, with its title bar and its edges. What the console opens as.
    #[default]
    Windowed,
    /// Borderless, filling the display it is on.
    Full,
}

impl Screen {
    /// The state a window in this condition is in.
    pub fn from_is_full(is_full: bool) -> Self {
        if is_full {
            Self::Full
        } else {
            Self::Windowed
        }
    }

    /// Does this state fill the display?
    pub fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// What a word on the lane **asks for**, which is not the same type as what the window **is**.
///
/// 🚨 **The split is [`ScreenCmd::Toggle`]'s doing and it is not ceremony.** `full` and
/// `windowed` name a destination; `toggle` names a *relation to wherever you are*, so it cannot
/// become a [`Screen`] without being told the current one. Collapsing the two into one enum
/// would mean either dropping `toggle` — the word a person reaching for this actually wants —
/// or having a `Screen::Toggle` variant that is not a state a window can be in, which is the
/// kind of value that gets matched somewhere it should not be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenCmd {
    /// Fill the display.
    Full,
    /// Return to an ordinary window.
    Windowed,
    /// Whichever of the two the window is not.
    Toggle,
}

impl ScreenCmd {
    /// The state this command puts a window currently in `now` into.
    ///
    /// Total, and the identity cases are real answers rather than gaps: `full` while full is
    /// [`Screen::Full`], which is what lets the caller compare and touch the window only on a
    /// genuine change.
    pub fn apply_to(self, now: Screen) -> Screen {
        match self {
            Self::Full => Screen::Full,
            Self::Windowed => Screen::Windowed,
            Self::Toggle => Screen::from_is_full(!now.is_full()),
        }
    }

    /// The word this command is spelled as on the sidecar — its entry in [`SCREEN_WORDS`].
    pub fn as_word(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Windowed => "windowed",
            Self::Toggle => "toggle",
        }
    }

    /// The command a word names, or a refusal carrying the words that would have worked.
    ///
    /// ⚠️ **No case folding and no prefixes** — [`crate::posture::Posture::resolve`]'s rule
    /// and [`crate::theme::Theme::resolve`]'s before it. An approximation here would resize a
    /// window somebody is looking at, to a state they did not name.
    ///
    /// Unlike a posture there is **no scalar spelling**, and the absence is the whole design
    /// argument in one line: a window either covers the display or it does not, and there is
    /// nothing in between for a number to address.
    pub fn resolve(word: &str) -> Result<Self, UnknownScreen> {
        match word {
            "full" => Ok(Self::Full),
            "windowed" => Ok(Self::Windowed),
            "toggle" => Ok(Self::Toggle),
            _ => Err(UnknownScreen { word: word.to_string() }),
        }
    }
}

/// The screen words, in the order `--help` should list them.
///
/// One table, read by `bin/ctl.rs`, by the console's command schema and by
/// [`ScreenCmd::resolve`]'s refusal — [`crate::posture::POSTURE_WORDS`]' arrangement, for its
/// reason: a second hand-maintained copy is how a CLI comes to accept a word nothing can act
/// on.
pub const SCREEN_WORDS: &[&str] = &["full", "windowed", "toggle"];

/// A word no screen state answers to, carrying the words that do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownScreen {
    /// Exactly what was asked for, unmodified.
    pub word: String,
}

impl std::fmt::Display for UnknownScreen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is not a screen state — known states: {}",
            self.word,
            SCREEN_WORDS.join(", ")
        )
    }
}

/// **The chord that flips it**, and the console's first unconditional key claim outside the
/// `⌘` band.
///
/// `F11` because it is the platform convention *and* because it is provably unspoken for here
/// — the three tests at the bottom of this module are what make that a fact rather than a
/// belief.
pub const CHORD: egui::Key = egui::Key::F11;

/// Is this keystroke the full-screen chord? `repeat` is the flag off the same
/// `egui::Event::Key` the other two arguments come from.
///
/// ⚠️ **Bare only, `matches_exact`, never `matches_logically`** —
/// [`crate::theme_edit::edit_key`]'s
/// rule and for its reason. `matches_logically` would let `Ctrl+F11` and `Shift+F11` through as
/// their bare selves, silently claiming three more chords than this one asked for and taking
/// them away from whatever wants them next.
///
/// 🚨 **A key-repeat is NOT the chord, and this is the one filter that had to be here rather
/// than left to be looked at.** Holding a key produces a stream of `pressed: true` events on
/// most platforms, and egui marks them (`Event::Key::repeat`). Without this filter, resting a
/// finger on the key would flip the window once per repeat — the state after letting go
/// decided by the parity of however many events arrived, which is indistinguishable from the
/// chord being broken. **It matters more here than for any other chord in the console**:
/// `⌘T` on repeat opens tabs somebody can see and close, whereas this is the only way *out* of
/// a window with no title bar, so its failure mode is being unable to trust the escape hatch.
/// A toggle is also the worst possible verb to repeat — an absolute `full` would simply be
/// re-applied and the change guard would swallow it.
///
/// ⚠️ **This is deliberately NOT fixed for the `⌘` chords in the same change.** They read the
/// same event without the flag and have since they were written, so a held `⌘T` presumably
/// opens a run of tabs — but that is existing behaviour on a different key table, its right
/// answer may not be "ignore the repeat" (an autorepeating `⌘1` is arguably fine), and nobody
/// has reported it. It is filed separately rather than folded in here.
pub fn screen_key(key: egui::Key, mods: egui::Modifiers, repeat: bool) -> Option<ScreenCmd> {
    (!repeat && key == CHORD && mods.matches_exact(egui::Modifiers::NONE))
        .then_some(ScreenCmd::Toggle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    /// Every modifier combination this crate's key tables distinguish, so the three
    /// "nobody else claims the chord" tests below sweep the same ground.
    const MOD_SETS: &[Modifiers] = &[
        Modifiers::NONE,
        Modifiers::SHIFT,
        Modifiers::CTRL,
        Modifiers::ALT,
        Modifiers::COMMAND,
        Modifiers::MAC_CMD,
    ];

    #[test]
    fn the_words_resolve_and_nothing_else_does() {
        assert_eq!(ScreenCmd::resolve("full"), Ok(ScreenCmd::Full));
        assert_eq!(ScreenCmd::resolve("windowed"), Ok(ScreenCmd::Windowed));
        assert_eq!(ScreenCmd::resolve("toggle"), Ok(ScreenCmd::Toggle));
        for bad in ["Full", "FULL", "fullscreen", "full ", "on", "off", "1", "0.5", ""] {
            assert!(ScreenCmd::resolve(bad).is_err(), "`{bad}` resolved and must not");
        }
    }

    /// 🚨 **`SCREEN_WORDS` is the table three surfaces read, so it must be exactly the set
    /// [`ScreenCmd::resolve`] accepts** — a word here that nothing resolves is a CLI offering a
    /// state the console cannot reach, and a resolvable word missing from here is a state
    /// `--help` never mentions. Both directions, and the round trip through `as_word`.
    #[test]
    fn the_word_table_and_the_resolver_are_one_vocabulary() {
        for word in SCREEN_WORDS {
            let cmd = ScreenCmd::resolve(word)
                .unwrap_or_else(|_| panic!("`{word}` is unresolvable"));
            assert_eq!(cmd.as_word(), *word, "`{word}` does not spell itself back");
        }
        for cmd in [ScreenCmd::Full, ScreenCmd::Windowed, ScreenCmd::Toggle] {
            assert!(SCREEN_WORDS.contains(&cmd.as_word()), "{cmd:?} is unlisted");
        }
        assert_eq!(SCREEN_WORDS.len(), 3, "a word was added without a variant, or the reverse");
    }

    /// The two absolute words ignore where the window is; `toggle` is the only one that reads
    /// it. The identity cases are asserted on purpose — the caller compares the answer with the
    /// current state to decide whether to touch the window at all.
    #[test]
    fn toggle_is_the_only_word_that_depends_on_where_the_window_is() {
        for now in [Screen::Windowed, Screen::Full] {
            assert_eq!(ScreenCmd::Full.apply_to(now), Screen::Full, "from {now:?}");
            assert_eq!(ScreenCmd::Windowed.apply_to(now), Screen::Windowed, "from {now:?}");
        }
        assert_eq!(ScreenCmd::Toggle.apply_to(Screen::Windowed), Screen::Full);
        assert_eq!(ScreenCmd::Toggle.apply_to(Screen::Full), Screen::Windowed);
        // …and twice is where you started, which is the property the word names.
        let there_and_back =
            ScreenCmd::Toggle.apply_to(ScreenCmd::Toggle.apply_to(Screen::Windowed));
        assert_eq!(there_and_back, Screen::Windowed);
    }

    #[test]
    fn a_window_opens_windowed() {
        assert_eq!(Screen::default(), Screen::Windowed);
        assert_eq!(Screen::from_is_full(false), Screen::Windowed);
        assert_eq!(Screen::from_is_full(true), Screen::Full);
        assert!(!Screen::Windowed.is_full());
        assert!(Screen::Full.is_full());
    }

    #[test]
    fn the_chord_is_bare_and_only_bare() {
        assert_eq!(screen_key(CHORD, Modifiers::NONE, false), Some(ScreenCmd::Toggle));
        for mods in MOD_SETS.iter().filter(|m| !m.matches_exact(Modifiers::NONE)) {
            assert_eq!(screen_key(CHORD, *mods, false), None, "{mods:?} must not flip the window");
        }
        for key in [Key::F10, Key::F12, Key::Escape, Key::Enter, Key::A, Key::Tab] {
            assert_eq!(screen_key(key, Modifiers::NONE, false), None, "{key:?} is not the chord");
        }
    }

    /// 🚨 **Holding the key must not flip the window once per repeat.** The state after letting
    /// go would then be decided by the parity of however many events arrived — and this chord
    /// is the only way out of a window with no title bar, so an escape hatch whose result
    /// depends on how long a finger rested on it is worse than no chord. Pinned rather than
    /// left to be noticed on a real display, because a repeat stream is exactly what does not
    /// show up in a screenshot.
    #[test]
    fn a_held_key_flips_the_window_once_not_once_per_repeat() {
        assert_eq!(screen_key(CHORD, Modifiers::NONE, true), None, "a repeat is not a press");
        // The shape of a real hold: one genuine press, then a stream of repeats. Exactly one
        // toggle comes out of it however long the stream is.
        let hold = std::iter::once(false).chain(std::iter::repeat(true).take(40));
        let flips = hold.filter(|r| screen_key(CHORD, Modifiers::NONE, *r).is_some()).count();
        assert_eq!(flips, 1, "a hold produced {flips} toggles");
    }

    /// 🚨 **The measurement the header's argument rests on: the terminal has never sent this
    /// key to a child.** [`crate::term::encode_key`] falls through to `None` for every function
    /// key, so claiming the chord takes nothing away from `vim`, `htop` or anything else
    /// running in a tab — which is exactly what Escape could not have said. The day somebody
    /// maps the function keys for a TUI that wants them, this fails and the argument has to be
    /// made again rather than quietly stop being true.
    #[test]
    fn the_terminal_has_never_sent_the_chord_to_a_child() {
        for mods in MOD_SETS {
            for app_cursor in [false, true] {
                assert_eq!(
                    crate::term::encode_key(CHORD, *mods, app_cursor),
                    None,
                    "the PTY would receive {CHORD:?} under {mods:?} (app_cursor={app_cursor})"
                );
            }
        }
    }

    /// …and neither of the conversation front-end's two key tables wants it either, so the
    /// claim collides with nothing in the other pane type.
    #[test]
    fn no_other_key_table_in_the_console_claims_the_chord() {
        use crate::conversation_view::{palette_key, PaletteKey};
        use crate::theme_edit::{edit_key, EditKey};
        for mods in MOD_SETS {
            assert_eq!(palette_key(CHORD, *mods), PaletteKey::Ignore, "the panel, under {mods:?}");
            assert_eq!(edit_key(CHORD, *mods), EditKey::Ignore, "the editor, under {mods:?}");
        }
    }

    #[test]
    fn a_refusal_names_the_words_that_would_have_worked() {
        let e = ScreenCmd::resolve("fullscreen").expect_err("not a word");
        let text = e.to_string();
        assert!(text.contains("fullscreen"), "the refusal drops what was typed: {text}");
        for word in SCREEN_WORDS {
            assert!(text.contains(word), "`{word}` is missing from the refusal: {text}");
        }
    }
}

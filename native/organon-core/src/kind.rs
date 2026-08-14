//! # The console's **kind** vocabulary — one table, both front-ends
//!
//! A *kind* is a name the console resolves to something that can draw itself into a
//! rectangle. Nothing else: never a command, never a path (`doc/console_patch_protocol.md`
//! §3, and the same rule the lighting scene protocol follows one machine over).
//!
//! ## Why it lives in the spine
//!
//! The console has two front-ends and it had **three** copies of this taxonomy, reached
//! independently and already diverging in spelling. `doc/console_view_paradigm.md` §5 counted
//! two on 2026-08-13; the third turned up while unifying those, one layer in from the wire:
//!
//! | Concept | the wire | terminal paint | conversation |
//! |---|---|---|---|
//! | a live control panel | `cli::PatchKind::Panel` | `block_panel::PatchContent::Panel` | `conversation::ArtifactContent::Panel` |
//! | a picture the engine draws | `cli::PatchKind::Scene` | `block_panel::PatchContent::Scene` | `conversation::ArtifactContent::Surface` |
//!
//! Those live in **different crates** — `cli.rs` in the root crate, the other two in
//! `organon-console` — so the wire copy and the paint copies could not import each other, and
//! the only crate all of them can see is this one. That is not merely where it *fits*:
//! `organon-core` is host-free by construction (no `nih_plug`, no `wgpu`, no `egui`), and a
//! closed set of words plus the functions that resolve them needs none of those. `tabs.rs` is
//! the precedent — a UI *taxonomy* with no UI in it.
//!
//! ## What the two front-ends actually share, and what they do not
//!
//! **The kind is shared; the payload is not.** A patch *names* a kind and stops there — the
//! whole point of `doc/console_patch_protocol.md` is that a program which can print can ask
//! for a rectangle without being able to drive the machine, so a claim carries a word and no
//! description. An artifact names a kind **and describes it** (`PanelSpec`'s slider and
//! button names, `SurfaceSpec`'s summoning look), because a conversation's summoning path is
//! inside the console rather than out on a text lane.
//!
//! So this module is the **vocabulary only**, and each placement keeps its own payload carrier
//! answering `kind()` from it: `block_panel::PatchContent` for inline-in-a-terminal (a panel
//! there owns live widget state pinned to scrollback lines) and `conversation::ArtifactContent`
//! for inline-in-a-conversation (a panel there is a description the view keys state off).
//! Those two methods are the only places the spellings meet, and each is pinned by a test
//! rather than left to be read carefully.
//!
//! ⚠️ **The two front-ends spell the picture kind differently to a human, and this module
//! does not change that.** The terminal lane's word is `scene` — it is in `--help`, in the
//! `organon-cli` skill and on the sidecar wire, so it is frozen. The conversation's composer
//! command is `/surface`, which is a word a human's fingers already know. Unifying *those*
//! would be a deliberate, documented break of one of them, and this change is inert by
//! contract, so it unifies the **set of kinds** and leaves the two spellings where they are.
//! `CONSOLE_ARCHITECTURE.md` records the residual so the next tier decides rather than
//! rediscovers.
//!
//! ## No default here
//!
//! There deliberately is no `Default`. The patch lane *does* have a default — a sidecar line
//! with no third word was written before kinds existed and means `scene` — but that is a
//! **wire-compatibility rule belonging to that lane**, not a claim that `scene` is the
//! natural kind. It lives beside the parser that needs it (`cli::PATCH_DEFAULT_KIND`), where
//! its reason is readable. The conversation side has no such history and must not inherit an
//! answer to a question it never asked.

use std::fmt;

/// What a claimed rectangle or an inline artifact **shows**.
///
/// 🚨 **A name the console resolves — never a command, never a path.** The writer says what
/// *sort* of thing belongs in the space it asked for; which scene, which panel, and how
/// either is drawn are the console's business entirely. That asymmetry is why this is a
/// closed set of words rather than a payload: a claim that could carry a command would be a
/// claim that could carry anything.
///
/// It is also why the two arms are so unequal in what they *cost* and so equal in what they
/// *say*. On the terminal side everything up to the paint — the claim, the anchor arithmetic,
/// the per-pane ledger — is shared; the kind selects the last step and nothing before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The rendered scene, drawn by the engine. On the terminal side it is sampled through
    /// the claimed rows; in a conversation it is an element with its own texture.
    Scene,
    /// A live control panel: real widgets living in the flow, not a picture of them.
    Panel,
}

/// Every kind, in the order `--help` should list them.
///
/// ⚠️ **Hand-written, and a new variant has to be added here too.** The compiler forces a word
/// out of [`Kind::as_word`] and a match arm out of [`Kind::from_word`]; it cannot force an
/// entry into a list. `tests::every_word_round_trips_and_the_two_tables_agree` catches a
/// word that resolves to nothing and a kind whose word is not offered — which is the pair
/// that produces a CLI accepting something nothing can draw.
pub const ALL: &[Kind] = &[Kind::Scene, Kind::Panel];

/// The kind words, in [`ALL`]'s order.
///
/// One table, read by `bin/ctl.rs`'s possible-values parser, by the console's command schema,
/// by `--help` and by [`Kind::from_word`] — the arrangement `console background`'s materials
/// use, for the reason recorded there: a second hand-maintained copy is how a CLI comes to
/// accept a word nothing can draw.
pub const KIND_WORDS: &[&str] = &["scene", "panel"];

impl Kind {
    /// The word this kind travels as, on the wire and in `--help`.
    pub fn as_word(self) -> &'static str {
        match self {
            Kind::Scene => "scene",
            Kind::Panel => "panel",
        }
    }

    /// The kind a word names, or `None`.
    ///
    /// The `None` arm is what the sidecar drain wants: an unknown kind is a line to skip,
    /// exactly as an unknown verb is, which is what keeps a newer CLI talking to an older
    /// console degrading to "that op did nothing". Anywhere a human is on the other end,
    /// use [`Kind::resolve`] instead — `None` alone cannot say what *would* have worked.
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "scene" => Some(Kind::Scene),
            "panel" => Some(Kind::Panel),
            _ => None,
        }
    }

    /// The kind a word names, or a refusal **carrying the list that would have worked**.
    ///
    /// 🚨 **Refused, never approximated.** There is no nearest-match, no case folding and no
    /// prefix rule, for the reason the lighting scene protocol gives one machine over: a
    /// silent approximation is indistinguishable from success, and here it would paint the
    /// wrong object into a rectangle somebody else's output is holding open. The known list
    /// travels *with* the refusal because an error that only says "no" is a dead end — the
    /// caller cannot ask this build what it can draw.
    pub fn resolve(word: &str) -> Result<Self, UnknownKind> {
        Kind::from_word(word).ok_or_else(|| UnknownKind { word: word.to_string() })
    }
}

/// A word no kind answers to, carrying the words that do.
///
/// A type rather than a formatted `String` so that every caller prints the same sentence
/// from the same list: the console's command schema and the CLI both refuse an unknown kind,
/// and two hand-written messages would eventually name two different sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKind {
    /// Exactly what was asked for, unmodified — quoting it back is how a caller sees a
    /// stray quote or a trailing space it did not know it had sent.
    pub word: String,
}

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a kind — known kinds: {}", self.word, KIND_WORDS.join(", "))
    }
}

impl std::error::Error for UnknownKind {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two tables are the one place this vocabulary can silently go wrong: a word with no
    /// kind is a CLI offering something nothing can draw, and a kind with no word is a kind
    /// no one can ask for.
    #[test]
    fn every_word_round_trips_and_the_two_tables_agree() {
        assert_eq!(ALL.len(), KIND_WORDS.len(), "every kind is offered, and nothing else is");
        for word in KIND_WORDS {
            let kind = Kind::from_word(word).unwrap_or_else(|| panic!("`{word}` resolves"));
            assert_eq!(kind.as_word(), *word);
        }
        for kind in ALL {
            assert!(KIND_WORDS.contains(&kind.as_word()), "{kind:?} is offered by --help");
            assert_eq!(Kind::from_word(kind.as_word()), Some(*kind));
        }
    }

    /// The wire form is lowercase and there is no near miss. Both halves matter: the first is
    /// what the sidecar and clap agree on, the second is the rule that a guess is worse than
    /// a refusal.
    #[test]
    fn an_unknown_word_is_refused_and_never_approximated() {
        assert_eq!(Kind::from_word("nonsense"), None);
        assert_eq!(Kind::from_word("Panel"), None, "the wire form is lowercase");
        assert_eq!(Kind::from_word("scenes"), None, "no prefix rule");
        assert_eq!(Kind::from_word(""), None);
        assert_eq!(Kind::from_word(" scene"), None, "no trimming — the caller owns its bytes");
    }

    /// The bar Tier 1 is judged on: a refusal that names the known list. An error that only
    /// says "no" leaves the caller with nowhere to go, and this is the one place the answer
    /// to "what *can* this build draw?" is free to include.
    #[test]
    fn a_refusal_carries_every_known_word() {
        let err = Kind::resolve("media").expect_err("`media` is not a kind in this build");
        assert_eq!(err.word, "media");
        let sentence = err.to_string();
        assert!(sentence.contains("`media`"), "it quotes back exactly what was asked for");
        for word in KIND_WORDS {
            assert!(sentence.contains(word), "the refusal names `{word}`: {sentence}");
        }
    }

    /// The happy arm of the same function, so `resolve` cannot quietly become "always
    /// refuse" — a failure the test above would not see.
    #[test]
    fn a_known_word_resolves_through_the_refusing_path_too() {
        for kind in ALL {
            assert_eq!(Kind::resolve(kind.as_word()), Ok(*kind));
        }
    }
}

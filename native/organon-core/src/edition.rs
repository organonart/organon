//! Build-time **product editions** (#483 Tier 1 — the Organon Mind shell).
//!
//! One crate family, three products. Organon (the VST3/CLAP visualizer), **Organon
//! Mind** (a standalone analysis instrument for local LLMs — see
//! `doc/organon_mind_prd.md`) and **Organon Shell** (the agent-operating workstation —
//! see `doc/organon_shell_prd.md`, private annex; its code is `native/organon-shell`)
//! are the *same* codebase with a different front-of-house, selected at **build time**
//! by the `mind-edition` / `shell-edition` cargo features (mutually exclusive,
//! enforced by a `compile_error!` below). This is an edition, **not a fork**: the
//! algorithm (`math.rs`), the shaders and the `Shared` snapshot are identical between
//! them, so the two products interoperate at every seam that matters.
//!
//! ⚠️ **This paragraph used to add "and — critically — the separate-process visual binary
//! are byte-identical between them", and that stopped being true in #554 Tier 4** (caught
//! on the #575 Mac pass, ~9 months after it went stale). It is corrected rather than
//! deleted because the *reason* is the point: a claim written when it was true stays
//! written after it isn't, and nothing recompiles a comment. The list below is the
//! measured one — `grep -rn 'EDITION\.|is_mind()' src/` — not the remembered one.
//!
//! The feature is **OFF by default**, so `cargo build` / `cargo test` / `bundle.sh` /
//! `deploy.sh` all keep producing exactly today's Organon. Everything below is a pure
//! function of the `Edition` value, so both editions' behaviour is unit-tested from a
//! default (feature-off) build — the fork is verified here, not just on the Mac.
//!
//! An edition drove exactly three things at #483 Tier 1; it now drives **six**. The first
//! three are the editor's, and are what "front-of-house" meant originally:
//! 1. **Branding** — the product name / window title (`product_name`). Also titles the
//!    visual's in-window HUD (`ui_layer::hud`).
//! 2. **The IPC namespace** — the `$TMPDIR` filename prefix every mmap + sidecar hangs
//!    off (`ipc_namespace`), so a Mind session and a full-Organon session can run side
//!    by side without stomping each other's snapshot. See `ipc::namespace`.
//! 3. **Which `UiTab`s the editor shows, and in what order** (`visible_tabs` /
//!    `shows_tab` / `default_tab`) — Organon Mind drops the Generator tab (its generator
//!    is always a neural network) plus Synth and Audio, and leads with Mind.
//!
//! The next two are **the visual binary's**, added by #554 Tier 4, and are why it is no
//! longer edition-independent:
//! 4. **Whether the window is an instrument or a projector feed**
//!    (`bin/visual.rs`'s `instrument_window`). Mind's visual comes up with an interface
//!    drawn in it, so it does *not* take the automatic second-display grab; full
//!    Organon's still does. An explicit `ORGANON_VISUAL_DISPLAY` wins in both.
//! 5. **Whether the in-window UI layer starts visible** (`ui_layer::UiLayer::new` →
//!    `visible`) — on in Mind, off in full Organon, **U** toggles either way.
//!
//! The sixth is **the library's own module tree**, added by #572's world hoist:
//! 6. **`pub mod world`** is `#[cfg(feature = "mind-edition")]`. Route C's editor needs
//!    the renderer reachable from `lib.rs`; a default build must not grow the plugin
//!    cdylib for it (measured: +490 KB ungated, 0 bytes gated). So the *library* differs
//!    between editions too, not only the front-of-house.
//!
//! Nothing else forks. In particular the **VST3 class ID / CLAP ID are untouched**
//! (Organon Mind is standalone-only, so it needs no plugin identity at all), and the
//! preset store is shared — a look saved in Organon is a look Organon Mind can pick.

use crate::tabs::UiTab;

/// Which product this build is. Compile-time only — there is no runtime switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edition {
    /// Organon — the full visualizer (VST3/CLAP plugin + standalone + visual).
    Full,
    /// Organon Mind — the standalone LLM-analysis instrument (`--features mind-edition`).
    Mind,
    /// Organon Shell — the agent-operating workstation (`--features shell-edition`,
    /// Shell #3 T1). Standalone-only like Mind: no plugin identity, ever.
    Shell,
}

// One build is one product. The edition features select a single `EDITION` const, so
// enabling two at once has no coherent meaning — fail loudly instead of letting cargo
// feature-unification pick a winner nobody chose.
#[cfg(all(feature = "mind-edition", feature = "shell-edition"))]
compile_error!(
    "`mind-edition` and `shell-edition` are mutually exclusive — one build is one product"
);

/// The edition **this build** is. Selected by the `mind-edition` / `shell-edition`
/// cargo features (default: neither → full Organon).
#[cfg(not(any(feature = "mind-edition", feature = "shell-edition")))]
pub const EDITION: Edition = Edition::Full;

/// The edition **this build** is. Selected by the `mind-edition` cargo feature.
#[cfg(all(feature = "mind-edition", not(feature = "shell-edition")))]
pub const EDITION: Edition = Edition::Mind;

/// The edition **this build** is. Selected by the `shell-edition` cargo feature.
#[cfg(all(feature = "shell-edition", not(feature = "mind-edition")))]
pub const EDITION: Edition = Edition::Shell;

/// The `UiTab`s Organon Mind shows, **in its own display order** (#520 Tier 1).
///
/// Mind wants most of Organon's functionality *rearranged*, not less of it. The only
/// tab that genuinely goes is **Generator** — Organon Mind's generator is always a
/// neural network, so the algorithm picker is meaningless and the Neural Network card
/// leads the Mind tab instead. Look / Motion / Environment all earn their place: this
/// is still the same renderer, and the analysis instrument wants to be beautiful.
/// **Synth and Audio** stay out (Mind observes a model, it doesn't make sound).
///
/// Note the **order deliberately differs from full Organon's** — Mind leads with Mind
/// and puts Look second. `visible_tabs` returns this slice as-is and the tab bar
/// iterates it, so this constant *is* the tab bar.
const MIND_TABS: &[UiTab] =
    &[UiTab::Mind, UiTab::Look, UiTab::Motion, UiTab::Environment, UiTab::Settings];

/// The `UiTab`s Organon Shell would show (Shell #3 T1).
///
/// **Provisional, and honestly so:** Shell's front-of-house is its own workspace (the
/// compositor in `native/organon-shell`), not the plugin editor, so no Shell binary
/// draws a tab bar today. The set still needs a defined answer — every edition-shaped
/// decision is a pure function of the `Edition` value — and the defensible one is the
/// look-shaping tabs an embedded editor pane would want: the same slice Mind ships,
/// minus the Mind lane. Revisit when a Shell surface actually embeds the editor UI.
const SHELL_TABS: &[UiTab] =
    &[UiTab::Look, UiTab::Motion, UiTab::Environment, UiTab::Settings];

impl Edition {
    /// The product name — the host/plugin name, the standalone window title, and the
    /// editor's heading. **`Full` must stay exactly `"Organon"`**: it feeds
    /// `Plugin::NAME`, which the host uses to identify the device.
    pub const fn product_name(self) -> &'static str {
        match self {
            Edition::Full => "Organon",
            Edition::Mind => "Organon Mind",
            Edition::Shell => "Organon Shell",
        }
    }

    /// One-line description of the product, for the Mind-edition header strip.
    ///
    /// **Shell's is deliberately empty** — the console says nothing about itself
    /// (James, 2026-08-11: the previous line here was agent-invented copy, and no
    /// call site may reintroduce one). The empty string is the contract, not a
    /// placeholder: a caller that would render it must skip it instead.
    pub const fn tagline(self) -> &'static str {
        match self {
            Edition::Full => "parametric 3D generative visualizer",
            Edition::Mind => "an instrument for watching a language model think",
            Edition::Shell => "",
        }
    }

    /// The **IPC namespace**: the `$TMPDIR` filename prefix every mmap + sidecar hangs
    /// off (`<namespace>-ipc.bin`, `<namespace>-mind.bin`, `<namespace>-model.txt`, …).
    ///
    /// **`Full` is frozen at `"organic-math"`** — the historical prefix. Changing it
    /// would silently break every already-running visual/runtime pairing and every
    /// documented `$TMPDIR` path (the sidecar names are part of the CLI/runtime
    /// contract), so this string is load-bearing in the same spirit as the VST3 class
    /// ID. Mind gets its own namespace, which is the whole point: two products, two
    /// snapshots, no collision.
    pub const fn ipc_namespace(self) -> &'static str {
        match self {
            Edition::Full => "organic-math",
            Edition::Mind => "organon-mind",
            Edition::Shell => "organon-shell",
        }
    }

    /// Does this edition's editor show `tab`?
    pub fn shows_tab(self, tab: UiTab) -> bool {
        match self {
            Edition::Full => true,
            Edition::Mind => MIND_TABS.contains(&tab),
            Edition::Shell => SHELL_TABS.contains(&tab),
        }
    }

    /// The tabs this edition shows, in display order.
    pub fn visible_tabs(self) -> &'static [UiTab] {
        match self {
            Edition::Full => UiTab::ALL,
            Edition::Mind => MIND_TABS,
            Edition::Shell => SHELL_TABS,
        }
    }

    /// The tab the editor opens on (and falls back to if the active tab is hidden —
    /// e.g. a preset saved under full Organon recalled inside Organon Mind).
    pub const fn default_tab(self) -> UiTab {
        match self {
            Edition::Full => UiTab::Generator,
            Edition::Mind => UiTab::Mind,
            Edition::Shell => UiTab::Look,
        }
    }

    /// `true` for the Mind edition — the one-liner UI code branches on.
    pub const fn is_mind(self) -> bool {
        matches!(self, Edition::Mind)
    }

    /// `true` for the Shell edition (Shell #3 T1) — the one-liner the
    /// workstation code branches on.
    pub const fn is_shell(self) -> bool {
        matches!(self, Edition::Shell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legacy prefix is frozen: every `$TMPDIR` path in `ipc.rs`, every documented
    /// sidecar name, and every running visual depend on it. A change here is a silent
    /// break, so pin it.
    #[test]
    fn full_namespace_is_the_frozen_legacy_prefix() {
        assert_eq!(Edition::Full.ipc_namespace(), "organic-math");
        assert_eq!(Edition::Full.product_name(), "Organon");
    }

    /// The whole point of the fork: no two editions ever share a mmap or a sidecar.
    /// Pairwise across all three — this is what lets a Shell session, a Mind session
    /// and a full-Organon session run simultaneously.
    #[test]
    fn editions_have_distinct_ipc_namespaces() {
        let all = [Edition::Full, Edition::Mind, Edition::Shell];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.ipc_namespace(), b.ipc_namespace(), "{a:?} vs {b:?}");
                assert_ne!(a.product_name(), b.product_name(), "{a:?} vs {b:?}");
            }
        }
    }

    /// The namespace becomes a `$TMPDIR` *filename component*, so it must not be able
    /// to reach outside it or confuse a shell/glob.
    #[test]
    fn namespaces_are_filename_safe() {
        for e in [Edition::Full, Edition::Mind, Edition::Shell] {
            let ns = e.ipc_namespace();
            assert!(!ns.is_empty());
            assert!(
                ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "namespace {ns:?} is not filename-safe"
            );
        }
    }

    /// Full Organon is unchanged: every tab it has today still shows.
    #[test]
    fn full_edition_shows_every_tab() {
        for &t in UiTab::ALL {
            assert!(Edition::Full.shows_tab(t), "{t:?} hidden in the full edition");
        }
        assert_eq!(Edition::Full.visible_tabs().len(), UiTab::ALL.len());
    }

    /// Organon Mind ships the Mind lane plus the shared look/motion/world tabs, in its
    /// own order (#520 Tier 1). The Generator tab goes (its generator is always a neural
    /// network), and so do Synth + Audio (Mind observes a model, it doesn't make sound).
    #[test]
    fn mind_edition_ships_the_workstation_tabs() {
        for t in [
            UiTab::Mind,
            UiTab::Look,
            UiTab::Motion,
            UiTab::Environment,
            UiTab::Settings,
        ] {
            assert!(Edition::Mind.shows_tab(t), "{t:?} should ship in Organon Mind");
        }
        for t in [UiTab::Generator, UiTab::Synth, UiTab::Audio] {
            assert!(!Edition::Mind.shows_tab(t), "{t:?} should be hidden in Organon Mind");
        }
    }

    /// The order is the product decision, not an accident of `UiTab::ALL` — Mind leads
    /// with Mind and puts Look second, which is NOT full Organon's order.
    #[test]
    fn mind_tab_order_is_mind_look_motion_environment_settings() {
        assert_eq!(
            Edition::Mind.visible_tabs(),
            &[UiTab::Mind, UiTab::Look, UiTab::Motion, UiTab::Environment, UiTab::Settings]
        );
        assert_ne!(
            Edition::Mind.visible_tabs().first(),
            Edition::Full.visible_tabs().first(),
            "the two editions should lead with different tabs"
        );
    }

    /// Whatever the editor opens on must actually be visible, or the first frame is blank.
    #[test]
    fn default_tab_is_always_visible() {
        for e in [Edition::Full, Edition::Mind, Edition::Shell] {
            assert!(e.shows_tab(e.default_tab()), "{e:?}'s default tab is hidden");
        }
        assert_eq!(Edition::Mind.default_tab(), UiTab::Mind);
        assert_eq!(Edition::Full.default_tab(), UiTab::Generator);
        assert_eq!(Edition::Shell.default_tab(), UiTab::Look);
    }

    /// `visible_tabs` and `shows_tab` are two views of one decision — keep them agreeing.
    #[test]
    fn visible_tabs_agrees_with_shows_tab() {
        for e in [Edition::Full, Edition::Mind, Edition::Shell] {
            for &t in UiTab::ALL {
                assert_eq!(
                    e.shows_tab(t),
                    e.visible_tabs().contains(&t),
                    "{e:?} disagrees about {t:?}"
                );
            }
        }
    }

    /// Organon Shell's provisional tab answer (Shell #3 T1): the look-shaping
    /// tabs, no Mind lane, no Generator/Synth/Audio — and its own name + namespace.
    #[test]
    fn shell_edition_identity_and_tabs() {
        assert_eq!(Edition::Shell.product_name(), "Organon Shell");
        assert_eq!(Edition::Shell.ipc_namespace(), "organon-shell");
        assert!(Edition::Shell.is_shell());
        assert!(!Edition::Shell.is_mind());
        assert!(!Edition::Full.is_shell());
        assert_eq!(
            Edition::Shell.visible_tabs(),
            &[UiTab::Look, UiTab::Motion, UiTab::Environment, UiTab::Settings]
        );
        for t in [UiTab::Mind, UiTab::Generator, UiTab::Synth, UiTab::Audio] {
            assert!(!Edition::Shell.shows_tab(t), "{t:?} should be hidden in Organon Shell");
        }
    }

    /// A default (feature-off) build is full Organon — the default-inert contract.
    #[test]
    fn default_build_is_the_full_edition() {
        #[cfg(not(any(feature = "mind-edition", feature = "shell-edition")))]
        assert_eq!(EDITION, Edition::Full);
        #[cfg(feature = "mind-edition")]
        assert_eq!(EDITION, Edition::Mind);
        #[cfg(feature = "shell-edition")]
        assert_eq!(EDITION, Edition::Shell);
    }
}

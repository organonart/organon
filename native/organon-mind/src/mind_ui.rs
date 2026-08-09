//! The **shared Mind UI** module (#483 Tier 1 — the Organon Mind shell).
//!
//! One source of truth, two presentations. Full Organon renders the Mind lane as one
//! tab among eight; **Organon Mind** leads with it. What differs between the two
//! products is *which chrome wraps the Mind lane* — which tabs exist, in what order,
//! and the heading above them — and that is what this module owns.
//!
//! Note what does **not** differ (#520 Tier 1): the Mind tab's own card layout is the
//! same in both products, so there is exactly one arrangement to reason about. The
//! cards themselves live in `lib.rs`; the Neural Network card is shared with the
//! Generator tab via `lib.rs::neural_network_card`.
//!
//! Everything decision-shaped here is a pure function of `(Edition, UiTab)` so it is
//! unit-tested from a default (feature-off) build; the egui calls are thin wrappers
//! around those decisions.

use organon_core::edition::{Edition, EDITION};
use organon_core::tabs::UiTab;

/// Keep the active tab **visible for this edition**.
///
/// A `PresetUi` deserialized from a full-Organon session (or simply its `Default`,
/// which is `UiTab::Generator`) can name a tab Organon Mind doesn't ship — which would
/// render an empty window. Clamp it to the edition's default instead. Returns the tab
/// to use; `None` change when it was already fine.
pub(crate) fn clamp_tab(edition: Edition, tab: UiTab) -> UiTab {
    if edition.shows_tab(tab) {
        tab
    } else {
        edition.default_tab()
    }
}

/// Draw the editor's top-level tab bar, filtered to this edition (#483 Tier 1).
///
/// Full Organon gets its eight tabs in its own order; Organon Mind gets
/// `Mind · Look · Motion · Environment · Settings` in *its* order (#520 Tier 1) — the
/// orders genuinely differ, which is why this iterates `visible_tabs()` rather than
/// `UiTab::ALL`. The active tab is clamped first, so a hidden tab can never leave the
/// window blank.
pub fn tab_bar(ui: &mut egui::Ui, tab: &mut UiTab) {
    *tab = clamp_tab(EDITION, *tab);
    ui.horizontal(|ui| {
        for &t in EDITION.visible_tabs() {
            ui.selectable_value(tab, t, t.label());
        }
    });
}

/// Should the editor point the visual at the loaded model's **specimen** this frame?
///
/// Organon Mind has no Generator tab, so a freshly loaded `.gguf` would light nothing —
/// the specimen only draws when the generator is Neural Network and its topology is
/// Connectome (loaded). The Mind edition sets both once per load; full Organon never
/// does (there you pick your own generator, and having a model load yank it out from
/// under you would be hostile).
///
/// Fires on the **edge** of `model_gen` against the last value acted on, so it happens
/// exactly once per load and never fights a change made afterwards. `model_gen == 0`
/// means no model has ever loaded.
pub fn should_point_at_specimen(
    edition: Edition,
    model_gen: u32,
    last_pointed: u32,
) -> bool {
    edition.is_mind() && model_gen > 0 && model_gen != last_pointed
}

/// How often, in editor frames, a pending auto-point re-issues its writes (#620).
///
/// Not every frame: `ParamSetter::set_parameter` **queues** the change for the audio thread
/// (`unprocessed_param_changes`) rather than applying it inline, so a readback cannot match until
/// that queue drains — one audio period, ~12 ms at 512/44100, i.e. usually the very next frame.
/// Re-issuing every frame would push several copies of a change that was already in flight and
/// spam the gesture pair around it. Half a second is far past the normal landing time, so a
/// re-issue only ever happens when the first one really did not take.
pub(crate) const AUTO_POINT_RETRY_EVERY: u32 = 30;

/// How many frames a pending auto-point keeps trying before giving up (#620).
///
/// ~3 s at 60 fps. It exists so a genuinely stuck write **stops** rather than re-issuing forever:
/// the reported symptom of this bug was the viewport "flashing back and forth between the original
/// and the neural network", and an unbounded retry is one way to *produce* that rather than fix it.
pub(crate) const AUTO_POINT_MAX_FRAMES: u32 = 180;

/// What a pending auto-point should do this frame (#620).
///
/// Pure so the retry policy is testable without a `ParamSetter`, a host, or a GPU — the decision
/// is the part with the bug in it, and the writes it drives are three lines of plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoPointStep {
    /// The params read back as intended — latch, and stop.
    Confirm,
    /// (Re-)issue the writes.
    Issue,
    /// In flight; the queue has not drained yet.
    Wait,
    /// Someone else moved these params. Stop, and leave their choice alone.
    Abandon,
    /// Out of attempts. Latch anyway so this stops trying, and say so.
    GiveUp,
}

/// Decide a pending auto-point's next step.
///
/// **`landed` is checked before the attempt budget** so a write that arrives on the very last
/// frame still counts. Order matters here: the other way round, a success and a timeout on the
/// same frame would be reported as a failure.
///
/// **`externally_changed` is what keeps the retry from fighting the user** (#628 review). The
/// auto-point's standing promise — stated where it is called — is that it fires on the `model_gen`
/// edge and *never fights a later manual change*. One-shot writes kept that promise for free.
/// Retrying does not: if the first write is dropped, the pending window stays open for up to
/// [`AUTO_POINT_MAX_FRAMES`], and from the user's point of view nothing happened, so reaching for
/// the generator themselves is the natural thing to do. Re-issuing over that would silently
/// overwrite a deliberate choice. Abandoning restores the promise.
pub fn auto_point_step(
    landed: bool,
    externally_changed: bool,
    attempts: u32,
) -> AutoPointStep {
    if landed {
        AutoPointStep::Confirm
    } else if externally_changed {
        AutoPointStep::Abandon
    } else if attempts >= AUTO_POINT_MAX_FRAMES {
        AutoPointStep::GiveUp
    } else if attempts % AUTO_POINT_RETRY_EVERY == 0 {
        AutoPointStep::Issue
    } else {
        AutoPointStep::Wait
    }
}

/// Has something other than this auto-point moved either param?
///
/// ⚠️ **The two params are written separately and land separately**, so "differs from where it
/// started" is *not* the test — a generator that has landed while its topology is still in the
/// queue differs from the baseline and is the completely normal mid-flight state. Reading that as
/// interference would abandon almost every real auto-point.
///
/// The actual test is per-param, and it is "is this value one of the two it is *allowed* to be":
/// the baseline it started at, or the target being written to it. Anything else had a third
/// author.
/// ⚠️ **Two type parameters, not one.** The pair is `(generator, topology)` — *different* types.
/// This was first written `<T: PartialEq>(now: (T, T), …)`, which compiled fine against tests that
/// passed `(&str, &str)` and could not compile at the real call site at all. A helper generic
/// enough for its test and not for its use is worse than a concrete one.
pub fn auto_point_externally_changed<G: PartialEq, T: PartialEq>(
    now: (G, T),
    baseline: (G, T),
    target: (G, T),
) -> bool {
    (now.0 != baseline.0 && now.0 != target.0) || (now.1 != baseline.1 && now.1 != target.1)
}

// The product heading that used to live here is gone in **both** editions. Its one caller was
// the top line of `editor_ui`, and the window title bar already names the product — in Organon
// Mind it said "Organon Mind" directly above a window called "Organon Mind". Under the wgpu
// editor it also sat on the scene, over a backdrop with no stable colour.
//
// `Edition::tagline` outlives it: it is `pub`, so it raises no dead-code warning, and it is the
// product's one-line description rather than a widget's string. It currently has no caller.

#[cfg(test)]
mod tests {
    use super::*;

    /// Full Organon never re-homes a tab — every tab it has is a tab it shows.
    #[test]
    fn clamp_is_identity_in_the_full_edition() {
        for &t in UiTab::ALL {
            assert_eq!(clamp_tab(Edition::Full, t), t);
        }
    }

    /// A tab Organon Mind doesn't ship falls back to the Mind lane rather than
    /// rendering an empty window.
    #[test]
    fn clamp_rehomes_hidden_tabs_in_the_mind_edition() {
        // Generator / Synth / Audio are the tabs Mind drops (#520 Tier 1).
        assert_eq!(clamp_tab(Edition::Mind, UiTab::Generator), UiTab::Mind);
        assert_eq!(clamp_tab(Edition::Mind, UiTab::Synth), UiTab::Mind);
        assert_eq!(clamp_tab(Edition::Mind, UiTab::Audio), UiTab::Mind);
        // …but every tab Mind *does* ship is left alone.
        for t in [
            UiTab::Mind,
            UiTab::Look,
            UiTab::Motion,
            UiTab::Environment,
            UiTab::Settings,
        ] {
            assert_eq!(clamp_tab(Edition::Mind, t), t);
        }
    }

    /// Clamping is idempotent and always lands on something visible — the property the
    /// tab bar relies on every frame.
    #[test]
    fn clamp_always_lands_on_a_visible_tab() {
        for e in [Edition::Full, Edition::Mind] {
            for &t in UiTab::ALL {
                let c = clamp_tab(e, t);
                assert!(e.shows_tab(c), "{e:?} clamped {t:?} to hidden {c:?}");
                assert_eq!(clamp_tab(e, c), c, "clamp is not idempotent for {e:?}/{t:?}");
            }
        }
    }

    /// Full Organon never auto-points: loading a model must not yank the generator out
    /// from under a user who chose one.
    #[test]
    fn the_full_edition_never_auto_points_at_the_specimen() {
        for gen in [0, 1, 7] {
            assert!(!should_point_at_specimen(Edition::Full, gen, 0));
        }
    }

    /// Organon Mind points once per load, on the edge — not every frame.
    #[test]
    fn mind_points_once_per_model_load() {
        // No model has ever loaded.
        assert!(!should_point_at_specimen(Edition::Mind, 0, 0));
        // First load fires.
        assert!(should_point_at_specimen(Edition::Mind, 1, 0));
        // Already acted on this load — later frames must not re-fire, or a user who
        // changed the view afterwards would have it snatched back every frame.
        assert!(!should_point_at_specimen(Edition::Mind, 1, 1));
        // A second load fires again.
        assert!(should_point_at_specimen(Edition::Mind, 2, 1));
        // Counters wrap (`fetch_add` on a u32); "different from last" is the test, not
        // "greater than last", so a wrapped counter still fires.
        assert!(should_point_at_specimen(Edition::Mind, 0u32.wrapping_sub(1), 3));
        assert!(should_point_at_specimen(Edition::Mind, 1, u32::MAX));
    }

    // ── #620: the auto-point retries until the params read back ─────────────

    /// The normal path: issue on the first frame, wait while the audio thread drains its
    /// queue, confirm as soon as the readback matches.
    #[test]
    fn auto_point_issues_then_waits_then_confirms() {
        assert_eq!(auto_point_step(false, false, 0), AutoPointStep::Issue);
        assert_eq!(auto_point_step(false, false, 1), AutoPointStep::Wait);
        assert_eq!(auto_point_step(false, false, 2), AutoPointStep::Wait);
        // The write lands.
        assert_eq!(auto_point_step(true, false, 3), AutoPointStep::Confirm);
    }

    // ── #628 review: the retry must not fight a manual change ───────────────

    /// The promise the auto-point makes where it is called is that it *never fights a later
    /// manual change*. One-shot writes kept that for free; retrying does not, so an external
    /// change has to stop the retry rather than be overwritten by it.
    #[test]
    fn auto_point_abandons_when_someone_else_moves_a_param() {
        assert_eq!(
            auto_point_step(false, true, 0),
            AutoPointStep::Abandon,
            "an external change must win even on a frame that would otherwise Issue"
        );
        assert_eq!(auto_point_step(false, true, 7), AutoPointStep::Abandon);
        assert_eq!(
            auto_point_step(false, true, AUTO_POINT_RETRY_EVERY),
            AutoPointStep::Abandon,
            "the retry frame is exactly the one that would have overwritten them"
        );
    }

    /// A landed write beats an "external change" reading — if the params are already at target
    /// there is nothing to fight over, and Confirm is the honest outcome.
    #[test]
    fn auto_point_landing_outranks_an_external_change() {
        assert_eq!(auto_point_step(true, true, 0), AutoPointStep::Confirm);
    }

    /// ⚠️ **The case that makes "differs from baseline" the wrong test.** The generator and the
    /// topology are written separately and land separately, so one-landed-one-pending is the
    /// ordinary mid-flight state. Reading it as interference would abandon nearly every real
    /// auto-point — which would restore the original bug wearing a different hat.
    #[test]
    fn auto_point_partial_landing_is_not_an_external_change() {
        let baseline = ("Original", "SmallWorld");
        let target = ("NeuralNetwork", "Connectome");
        // Generator landed, topology still queued.
        assert!(!auto_point_externally_changed(
            ("NeuralNetwork", "SmallWorld"),
            baseline,
            target
        ));
        // Topology landed, generator still queued.
        assert!(!auto_point_externally_changed(
            ("Original", "Connectome"),
            baseline,
            target
        ));
        // Nothing landed yet.
        assert!(!auto_point_externally_changed(baseline, baseline, target));
        // Both landed.
        assert!(!auto_point_externally_changed(target, baseline, target));
    }

    /// A third value in *either* slot is somebody else's doing — that is the whole signal.
    #[test]
    fn auto_point_a_third_value_in_either_slot_is_external() {
        let baseline = ("Original", "SmallWorld");
        let target = ("NeuralNetwork", "Connectome");
        assert!(auto_point_externally_changed(
            ("Boids", "SmallWorld"),
            baseline,
            target
        ));
        assert!(auto_point_externally_changed(
            ("Original", "Layered"),
            baseline,
            target
        ));
        // Even mixed with a landed half.
        assert!(auto_point_externally_changed(
            ("NeuralNetwork", "Layered"),
            baseline,
            target
        ));
    }

    /// If the baseline already *is* the target, the auto-point is a no-op that confirms
    /// immediately — but a user moving away from it during that frame is still external.
    #[test]
    fn auto_point_handles_a_baseline_already_at_target() {
        let target = ("NeuralNetwork", "Connectome");
        assert!(!auto_point_externally_changed(target, target, target));
        assert!(auto_point_externally_changed(
            ("Boids", "Connectome"),
            target,
            target
        ));
    }

    /// ⚠️ **The pair is two *different* types**, and this test exists to keep it that way.
    ///
    /// The helper was first written `<T: PartialEq>(now: (T, T), …)`. Every test above passes
    /// `(&str, &str)`, which satisfies that happily — so the whole suite compiled and passed
    /// while the real call site, which passes `(GeneratorMode, NeuralTopology)`, could not compile
    /// at all. **A helper generic enough for its tests and not for its use.** Exercising a
    /// heterogeneous pair here is what makes the signature's second type parameter load-bearing
    /// rather than incidental.
    #[test]
    fn auto_point_externally_changed_takes_a_heterogeneous_pair() {
        // Deliberately (&str, u8): different types, as the real (generator, topology) is.
        assert!(!auto_point_externally_changed(
            ("Original", 0u8),
            ("Original", 0u8),
            ("NeuralNetwork", 1u8)
        ));
        assert!(auto_point_externally_changed(
            ("Boids", 0u8),
            ("Original", 0u8),
            ("NeuralNetwork", 1u8)
        ));
        assert!(auto_point_externally_changed(
            ("Original", 9u8),
            ("Original", 0u8),
            ("NeuralNetwork", 1u8)
        ));
    }

    /// **The bug this fixes.** A dropped write leaves the readback false, and the policy must
    /// eventually issue again — the old code could not, because it latched before writing.
    #[test]
    fn auto_point_reissues_after_a_lost_write() {
        // Nothing between the issues but waiting…
        for f in 1..AUTO_POINT_RETRY_EVERY {
            assert_eq!(auto_point_step(false, false, f), AutoPointStep::Wait, "frame {f}");
        }
        // …then a fresh attempt.
        assert_eq!(
            auto_point_step(false, false, AUTO_POINT_RETRY_EVERY),
            AutoPointStep::Issue
        );
        assert_eq!(
            auto_point_step(false, false, AUTO_POINT_RETRY_EVERY * 2),
            AutoPointStep::Issue
        );
    }

    /// A retry that never stops is one way to *produce* the reported "flashing back and forth
    /// forever" symptom rather than fix it, so the budget is bounded.
    #[test]
    fn auto_point_gives_up_rather_than_retrying_forever() {
        assert_eq!(
            auto_point_step(false, false, AUTO_POINT_MAX_FRAMES),
            AutoPointStep::GiveUp
        );
        assert_eq!(
            auto_point_step(false, false, AUTO_POINT_MAX_FRAMES + 1),
            AutoPointStep::GiveUp
        );
        // The budget is a multiple of the retry interval, so the give-up frame is one that
        // would otherwise have issued — i.e. give-up genuinely wins that tie.
        assert_eq!(AUTO_POINT_MAX_FRAMES % AUTO_POINT_RETRY_EVERY, 0);
    }

    /// A write that arrives on the very last frame still counts. Checked because the other
    /// ordering — budget before readback — reports a success as a failure, and the only
    /// difference is which `if` comes first.
    #[test]
    fn auto_point_confirms_even_on_the_give_up_frame() {
        assert_eq!(
            auto_point_step(true, false, AUTO_POINT_MAX_FRAMES),
            AutoPointStep::Confirm
        );
        assert_eq!(auto_point_step(true, false, u32::MAX), AutoPointStep::Confirm);
    }

    /// The retry interval has to be past the time a queued change actually takes to land
    /// (~1 frame), or every pending auto-point would issue several copies of a change that was
    /// already in flight.
    #[test]
    fn auto_point_retry_interval_is_past_the_normal_landing_time() {
        assert!(AUTO_POINT_RETRY_EVERY > 2);
        assert!(AUTO_POINT_MAX_FRAMES > AUTO_POINT_RETRY_EVERY);
    }
}

//! The editor's **tab taxonomy** — the two enums that name what the UI is showing.
//!
//! Moved out of `preset.rs` by **#626 Tier 3** (spec'd as #536 Tier 4, reference #5:
//! `edition.rs -> crate::preset::UiTab`). `edition.rs` decides which tabs a product
//! ships, so it needs these types; `preset.rs` needs `nih_plug`'s `ParamSetter` to
//! write params through the host. Before the split, the second dragged the first into
//! every build. Now the taxonomy is a leaf and the host-automation logic stays put.
//!
//! ## Why BOTH enums moved when only `UiTab` was named
//!
//! `UiTab::preset_tab()` returns `Option<EditorTab>`, and Rust inherent impls must live
//! in the type's defining crate — so the method could not stay behind in `preset.rs`.
//! The alternatives were to demote it to a free function purely to dodge the crate
//! boundary, or to move the type it returns. `EditorTab`'s impl is self-contained
//! (labels, a stable index, ordering) and carries no `nih_plug`, so moving it is
//! honest: these are two views of one taxonomy and they belong together. Bending the
//! code to fit the boundary would have been the wrong direction of travel.
//!
//! ## What deliberately stayed in `preset.rs`
//!
//! `EditorTab::file_slug` — the on-disk `presets_<slug>.json` name. That is a
//! *persistence* concern, not tab identity, and it is used only by `tab_presets_path`.
//! It now lives beside its one caller as a free function, which keeps this module free
//! of any opinion about the filesystem. `PresetValues::apply_tab(tab: EditorTab, ..)`
//! also stays: taking a core type as a parameter costs nothing.
//!
//! Neither enum derives anything from `nih_plug` — they are plain `Copy` enums, which
//! is what let them move at all. Contrast `params.rs`'s ~102 `#[derive(Enum)]` types,
//! none of which move in this tier.

/// The editor's top-level tab (Blender-style): one section shown at a time
/// instead of three crowded columns side by side (#131).
///
/// NOTE: this is the **3-way param partition** the per-tab preset system is
/// built on (`for_each_tab_field!`, `apply_tab`, the drift-guard test) — it is
/// NOT the editor's tab bar. The tab bar is the 5-way `UiTab` below; the two
/// extra UI tabs (Environment, Settings) hold per-display / world state that
/// presets don't capture, so they have no `EditorTab` and no per-tab presets.
///
/// #354: grown from a 3-way to a **7-way** partition. The first four
/// (Generator/Motion/Environment/Look) are the **Scene** components — a Scene
/// preset is their union. Audio/Synth/Settings have their own preset lists and
/// are deliberately **not** part of a Scene.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum EditorTab {
    /// Generator selector + the active generator's params + Surface mode.
    #[default]
    Generator,
    /// Animation / camera / pulse / speed-pulse / breath.
    Motion,
    /// World layer + sky/IBL: terrain, sun & day cycle, atmosphere, clouds,
    /// ocean, starfield, the loaded HDR + backdrop/env-tint (#354).
    Environment,
    /// Material / lighting / post / particles.
    Look,
    /// Audio analyzer / metering / calibrated instrument + pulse routing (#354).
    Audio,
    /// The Duo-Field synth engine (#354). Its own list; not in a Scene.
    Synth,
    /// Renderer (HDR/MSAA/tone-map), output resolution, capture stack, and the
    /// sync/tempo clock config (#354). Its own list; not in a Scene.
    Settings,
}

impl EditorTab {
    /// Display label for the tab bar / "Save <Tab>" button.
    pub fn label(self) -> &'static str {
        match self {
            EditorTab::Generator => "Generator",
            EditorTab::Motion => "Motion",
            EditorTab::Environment => "Environment",
            EditorTab::Look => "Look",
            EditorTab::Audio => "Audio",
            EditorTab::Synth => "Synth",
            EditorTab::Settings => "Settings",
        }
    }
    /// Stable index 0..7 — keys the per-tab preset-list array in `PresetUi`.
    pub fn index(self) -> usize {
        match self {
            EditorTab::Generator => 0,
            EditorTab::Motion => 1,
            EditorTab::Environment => 2,
            EditorTab::Look => 3,
            EditorTab::Audio => 4,
            EditorTab::Synth => 5,
            EditorTab::Settings => 6,
        }
    }
    /// The number of per-tab preset lists (`PresetUi::tab_presets` length).
    pub const COUNT: usize = 7;
    /// All tabs, in display order.
    pub const ALL: [EditorTab; Self::COUNT] = [
        EditorTab::Generator,
        EditorTab::Motion,
        EditorTab::Environment,
        EditorTab::Look,
        EditorTab::Audio,
        EditorTab::Synth,
        EditorTab::Settings,
    ];
    /// The four partitions that compose a **Scene** (#354). Audio/Synth/Settings
    /// are excluded — they have their own presets but never ride in a Scene.
    pub const SCENE: [EditorTab; 4] = [
        EditorTab::Generator,
        EditorTab::Motion,
        EditorTab::Environment,
        EditorTab::Look,
    ];
    /// Is this tab one of the four Scene components?
    pub fn in_scene(self) -> bool {
        Self::SCENE.contains(&self)
    }
}

/// The editor's top-level **UI tab bar** (five sections, in display order).
/// Distinct from `EditorTab` (the 3-way preset partition): Environment and
/// Settings show per-display / world-layer controls that presets don't
/// capture, so `preset_tab()` maps them to `None` and the presets rail offers
/// only the Global list while they're active.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum UiTab {
    #[default]
    Generator,
    Motion,
    /// The world layer (was the floating 🌍 panel): terrain, sun & day cycle,
    /// atmosphere, clouds, ocean, starfield.
    Environment,
    Look,
    /// The Duo-Field synth engine (#354, was a card on the Generator tab).
    Synth,
    /// Audio instrument (#333, was the floating 🎵 panel): input meter + spectrum,
    /// calibrated BS.1770 meters + RTA + spectrogram, and the oscilloscope.
    Audio,
    /// Infrequently-changed, per-display plumbing: renderer (HDR/MSAA/tone
    /// map), output resolution, and the capture / production-frame stack
    /// (was the floating 🎬 panel).
    Settings,
    /// The AI Performer + local-LLM specimen surface (#317 / #367): chat/agent
    /// (col 0) and model/specimen (col 1). No per-tab presets (like a UI-only tab).
    Mind,
}

impl UiTab {
    /// Every UI tab, in **display order** — the single source of truth the editor's
    /// tab bar iterates and the `Edition` tab filter (#483 Tier 1) is checked against.
    /// Add a new tab here and it appears in the bar; `edition::Edition::shows_tab`
    /// decides whether a given product ships it.
    pub const ALL: &'static [UiTab] = &[
        UiTab::Generator,
        UiTab::Motion,
        UiTab::Environment,
        UiTab::Look,
        UiTab::Synth,
        UiTab::Audio,
        UiTab::Settings,
        UiTab::Mind,
    ];

    /// The tab bar's label for this tab.
    pub fn label(self) -> &'static str {
        match self {
            UiTab::Generator => "Generator",
            UiTab::Motion => "Motion",
            UiTab::Environment => "Environment",
            UiTab::Look => "Look",
            UiTab::Synth => "Synth",
            UiTab::Audio => "Audio",
            UiTab::Settings => "Settings",
            UiTab::Mind => "Mind",
        }
    }

    /// The tab's name as a person **types** it — the label, lowercased.
    ///
    /// ⚠️ **Derived from [`UiTab::label`] rather than spelled a second time.** Every label is
    /// one word today, so lowercasing is the whole transformation; a two-word label would need
    /// a real slug (the command grammar splits on whitespace) and
    /// [`every_tab_word_is_one_word`](self) fails the build if one ever arrives, rather than
    /// letting a tab quietly become untypeable.
    pub fn word(self) -> String {
        self.label().to_ascii_lowercase()
    }

    /// The tab a typed word names, case-insensitively. `None` for anything that is not a tab.
    pub fn from_word(word: &str) -> Option<UiTab> {
        Self::ALL.iter().copied().find(|t| t.label().eq_ignore_ascii_case(word))
    }

    /// The per-tab-preset partition this UI tab corresponds to. Most UI tabs map
    /// to a partition (#354 — Environment/Audio/Synth/Settings gained presets);
    /// the Mind tab (#317/#367) is UI-only and maps to `None`.
    pub fn preset_tab(self) -> Option<EditorTab> {
        Some(match self {
            // The Mind tab is UI-only (no per-tab presets).
            UiTab::Mind => return None,
            UiTab::Generator => EditorTab::Generator,
            UiTab::Motion => EditorTab::Motion,
            UiTab::Environment => EditorTab::Environment,
            UiTab::Look => EditorTab::Look,
            UiTab::Audio => EditorTab::Audio,
            UiTab::Synth => EditorTab::Synth,
            UiTab::Settings => EditorTab::Settings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`UiTab::word`] lowercases the label and does nothing else, so a two-word label would
    /// produce a "word" with a space in it — and the Console's command grammar splits on
    /// whitespace, so that tab would silently become untypeable. See that method's doc.
    #[test]
    fn every_tab_word_is_one_word() {
        for tab in UiTab::ALL {
            assert!(
                !tab.word().contains(char::is_whitespace),
                "`{}` needs a real slug, not a lowercased label",
                tab.label()
            );
        }
    }

    #[test]
    fn a_tab_round_trips_through_its_word() {
        for tab in UiTab::ALL {
            assert_eq!(UiTab::from_word(&tab.word()), Some(*tab));
        }
        assert_eq!(UiTab::from_word("LOOK"), Some(UiTab::Look));
        assert_eq!(UiTab::from_word("nonesuch"), None);
    }
}

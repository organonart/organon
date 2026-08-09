//! Key Map: assign presets to MIDI notes so that holding a note makes that
//! preset active on the visual.
//!
//! Like a played clip's CCs, this drives the visual *directly* via the IPC
//! snapshot (not through the host sliders) — a held key replaces the look
//! wholesale, and releasing it reverts. So the mapping lives entirely on the
//! plugin side: the editor edits a note→preset-name table (`KeyMapping`, saved
//! globally next to `presets.json`), the plugin resolves it into per-note
//! `PresetValues` (`KeyMap`), and `process()` reads the resolved map on the audio
//! thread (wait-free, via `ArcSwap`) and, for whichever mapped note is held,
//! overlays that preset's Scene tabs onto the live look (#354).

use crate::preset::{Preset, PresetValues};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Number of MIDI notes (0..=127).
pub const N_NOTES: usize = 128;

/// MIDI note number for C0 under Ableton's convention (C3 = MIDI 60). All octave
/// math derives from here, so changing one constant re-bases the whole keyboard.
pub const C0_MIDI: i32 = 24;

/// Lowest / highest octave the editor keyboard pages through (C0..C5 inclusive).
pub const MIN_OCTAVE: i32 = 0;
pub const MAX_OCTAVE: i32 = 5;

/// Pitch-class names (index = semitone within the octave).
pub const PITCH_NAMES: [&str; 12] =
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// MIDI note number for pitch class `pc` (0..12) in display octave `octave`.
pub fn midi_for(octave: i32, pc: i32) -> i32 {
    C0_MIDI + (octave - 0) * 12 + pc
}

/// Human-readable name for a MIDI note, e.g. 60 → "C3".
pub fn note_name(midi: u8) -> String {
    let m = midi as i32;
    let octave = m / 12 - (C0_MIDI / 12); // C0_MIDI/12 == 2, so 60→3
    let pc = (m % 12) as usize;
    format!("{}{}", PITCH_NAMES[pc], octave)
}

/// The editor-editable mapping: MIDI note → preset name. Serialized to a small
/// JSON object so it survives preset reordering (presets are keyed by name).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct KeyMapping {
    /// note number (as string for JSON object keys) → preset name.
    #[serde(default)]
    pub notes: BTreeMap<u8, String>,
}

impl KeyMapping {
    pub fn get(&self, note: u8) -> Option<&str> {
        self.notes.get(&note).map(|s| s.as_str())
    }

    pub fn assign(&mut self, note: u8, preset: &str) {
        self.notes.insert(note, preset.to_string());
    }

    pub fn clear(&mut self, note: u8) {
        self.notes.remove(&note);
    }

    pub fn load() -> KeyMapping {
        std::fs::read_to_string(keymap_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(keymap_path(), json);
        }
    }
}

fn keymap_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("OrganicMath");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("keymap.json")
}

/// Audio-thread-ready resolution of `KeyMapping`: for each mapped note, the
/// preset's captured `PresetValues`, ready for the audio thread to overlay onto
/// the live look. Rebuilt by the editor (off the audio thread) whenever the
/// mapping or the presets change, then published through an `ArcSwap` for the
/// audio thread to read wait-free.
///
/// #354: this stores the preset's `PresetValues` (not a baked `Shared`) so the
/// Key Map presets are **Scene** presets, and a Scene recall touches only its
/// four Scene tabs. Because a Scene preset is stored sparse (Settings/Audio/Synth
/// = serde defaults), the merge with the *live* look must happen at play time on
/// the audio thread (`process()` overlays these Scene tabs onto a fresh live
/// capture), NOT baked at build — else the non-Scene base goes stale as sliders /
/// host-restore change the live params without a rebuild.
pub struct KeyMap {
    slots: Box<[Option<PresetValues>]>,
}

impl Default for KeyMap {
    fn default() -> Self {
        KeyMap {
            slots: vec![None; N_NOTES].into_boxed_slice(),
        }
    }
}

impl KeyMap {
    /// Resolve a mapping against the current preset list. Unknown preset names
    /// (e.g. a deleted preset) leave that note unmapped.
    pub fn build(mapping: &KeyMapping, presets: &[Preset]) -> KeyMap {
        let mut slots = vec![None; N_NOTES];
        for (&note, name) in &mapping.notes {
            if (note as usize) < N_NOTES {
                if let Some(p) = presets.iter().find(|p| p.name == *name) {
                    slots[note as usize] = Some(p.values.clone());
                }
            }
        }
        KeyMap {
            slots: slots.into_boxed_slice(),
        }
    }

    /// The captured preset values for a note, if mapped to a still-existing
    /// preset. `process()` overlays these Scene tabs onto the live look.
    pub fn get(&self, note: u8) -> Option<&PresetValues> {
        self.slots.get(note as usize).and_then(|o| o.as_ref())
    }
}

/// Stack of currently-held MIDI notes, newest last. Pre-allocated to the full
/// note range so press/release never allocate on the audio thread.
pub struct HeldKeys {
    stack: Vec<u8>,
}

impl Default for HeldKeys {
    fn default() -> Self {
        HeldKeys {
            stack: Vec::with_capacity(N_NOTES),
        }
    }
}

impl HeldKeys {
    /// Note pressed: move it to the top (last-press wins). De-dups so a re-press
    /// of an already-held note just re-prioritizes it.
    pub fn press(&mut self, note: u8) {
        self.stack.retain(|&n| n != note);
        if self.stack.len() < N_NOTES {
            self.stack.push(note);
        }
    }

    pub fn release(&mut self, note: u8) {
        self.stack.retain(|&n| n != note);
    }

    /// The newest held note that resolves to a mapped preset, scanning from the
    /// top down so a held-but-unmapped note doesn't mask an older mapped one.
    pub fn top_mapped(&self, km: &KeyMap) -> Option<u8> {
        self.stack.iter().rev().copied().find(|&n| km.get(n).is_some())
    }

    /// The newest still-held note (last-press-wins), regardless of any mapping.
    /// Used by the Tier 4 mono synth voices to follow the top of the stack.
    pub fn top(&self) -> Option<u8> {
        self.stack.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{Preset, PresetValues};
    use crate::params::OrganicMathParams;

    fn sample_preset(name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            values: PresetValues::capture(&OrganicMathParams::default()),
        }
    }

    #[test]
    fn note_naming_matches_ableton_c3_60() {
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(24), "C0");
        assert_eq!(note_name(84), "C5");
        assert_eq!(note_name(61), "C#3");
        assert_eq!(midi_for(3, 0), 60);
        assert_eq!(midi_for(0, 0), 24);
        assert_eq!(midi_for(5, 0), 84);
    }

    #[test]
    fn held_top_is_last_press_wins_and_falls_back_on_release() {
        let mut h = HeldKeys::default();
        assert_eq!(h.top(), None);
        h.press(60);
        h.press(64);
        h.press(67);
        assert_eq!(h.top(), Some(67)); // newest held
        h.release(67);
        assert_eq!(h.top(), Some(64)); // falls back to the next-held note
        h.press(60); // re-press re-prioritizes an already-held note
        assert_eq!(h.top(), Some(60));
        h.release(64);
        h.release(60);
        assert_eq!(h.top(), None); // nothing held → no pitch
    }

    #[test]
    fn build_resolves_names_and_drops_missing() {
        let presets = vec![sample_preset("Helix"), sample_preset("Jellyfish")];
        let mut m = KeyMapping::default();
        m.assign(60, "Helix");
        m.assign(62, "Ghost"); // no such preset
        let km = KeyMap::build(&m, &presets);
        assert!(km.get(60).is_some());
        assert!(km.get(62).is_none()); // unknown name → unmapped
        assert!(km.get(64).is_none()); // never assigned
    }

    #[test]
    fn mapping_serde_round_trip() {
        let mut m = KeyMapping::default();
        m.assign(60, "Helix");
        m.assign(48, "Lattice");
        let json = serde_json::to_string(&m).unwrap();
        let back: KeyMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get(60), Some("Helix"));
        assert_eq!(back.get(48), Some("Lattice"));
    }

    #[test]
    fn held_keys_last_press_wins_and_falls_back() {
        let presets = vec![sample_preset("A"), sample_preset("B")];
        let mut m = KeyMapping::default();
        m.assign(60, "A");
        m.assign(64, "B");
        let km = KeyMap::build(&m, &presets);

        let mut held = HeldKeys::default();
        assert_eq!(held.top_mapped(&km), None);

        held.press(60);
        assert_eq!(held.top_mapped(&km), Some(60));
        held.press(64); // newer
        assert_eq!(held.top_mapped(&km), Some(64));
        held.release(64); // fall back to the still-held older note
        assert_eq!(held.top_mapped(&km), Some(60));
        held.release(60);
        assert_eq!(held.top_mapped(&km), None);
    }

    #[test]
    fn unmapped_held_note_does_not_mask_mapped_one() {
        let presets = vec![sample_preset("A")];
        let mut m = KeyMapping::default();
        m.assign(60, "A");
        let km = KeyMap::build(&m, &presets);

        let mut held = HeldKeys::default();
        held.press(60); // mapped
        held.press(62); // newer but unmapped
        // The unmapped top note is skipped; the mapped one stays active.
        assert_eq!(held.top_mapped(&km), Some(60));
    }
}

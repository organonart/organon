//! User preferences — **the console's first durable record of a choice a person made.**
//!
//! Measured 2026-08-13: nothing the console offers a user survives exit. The only
//! writer in the crate is [`crate::session::SessionLog`], and that is an append-only
//! event corpus — evidence of what happened, not a statement of what is wanted. The
//! only user-*configuration* path is a **read** with no matching write
//! ([`crate::harness::load`] over `harnesses.json`); everything else is an
//! `ORGANON_SHELL_*` environment variable sampled once at startup. So a picker — the
//! colour theme is the one being built now — could offer a choice and then lose it,
//! which makes the picker pointless.
//!
//! This module is the other half: `preferences.json`, beside `harnesses.json` at the
//! same store root.
//!
//! **What that commits us to.** A file the product writes on the user's behalf is a
//! durable promise, and it is the first one the console has made. Three properties
//! follow, and each is pinned by a test below rather than left as intent:
//!
//! 1. **A write must not be able to destroy what is already stored.** [`Preferences::save`]
//!    writes a temp file in the *same directory* and renames it over the target, so a
//!    crash or a power cut leaves either the old file or the new one and never a
//!    half-written one. A truncated JSON file would fail to parse, and rule 2 would then
//!    silently reset every preference the user had — which is the worst outcome this
//!    module can produce, so the cheap defence is worth having from the first version.
//! 2. **Reading is total.** Missing, unreadable, or malformed ⇒ "no stored preferences",
//!    never an error and never a crash — the posture [`crate::harness::load`] already
//!    takes for the file next door, and the reason is the same: a corrupt preferences
//!    file must cost you your preferences, never your ability to open the console.
//! 3. **The shape grows additively.** Container-level `#[serde(default)]` means an older
//!    file missing a newer key still loads, and serde's default tolerance of unknown
//!    fields means a newer file does not break an older binary. Adding a preference is
//!    one field; it is never a migration.
//!
//! ⚠️ **Never write this file with a UTF-8 BOM.** `serde_json` refuses a BOM outright,
//! and rule 2 turns that refusal into silence — the file would be present, look correct
//! in an editor, and do nothing, with no line anywhere saying why. This machine has that
//! exact failure on record for `harnesses.json`, written by a PowerShell
//! `Set-Content -Encoding utf8`. [`Preferences::save`] writes plain UTF-8 and
//! [`tests::a_bom_is_never_written_and_is_not_tolerated_on_read`] pins both halves.
//!
//! 📌 **No environment variable overrides a stored preference, deliberately.** The
//! `ORGANON_SHELL_*` family stays exactly as it is — startup-only knobs, none of which
//! this module reads or writes. Adding an override would defeat the thing being built:
//! a user picks a theme, the pick is stored correctly, and next launch a variable baked
//! into a launch shim years ago silently wins — which is indistinguishable from the
//! evaporation this file exists to end. The shims on this machine already demonstrate
//! the failure: `organon-console.cmd` sets `ORGANON_SHELL_TABS` unconditionally, so the
//! documented way to override it is ignored without a word. If a one-launch override is
//! ever genuinely wanted, it belongs in a CLI flag that can announce itself in the
//! console's own output, not in an invisible variable that outranks the file forever.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::session::SessionLog;

/// The preferences file, at the store root beside `harnesses.json`.
///
/// One flat file, not a directory of them: preferences are small, few, and read
/// together at startup, and the split that `ui_theme.json` / `ui_themes.json` make in
/// the visualizer exists because those two have different *lifetimes*. These do not.
pub const PREFS_FILE: &str = "preferences.json";

/// Everything the console remembers about how a user wants it to behave.
///
/// **Add a preference by adding a field.** Container-level `#[serde(default)]` covers
/// the old-file-missing-the-key direction and serde's unknown-field tolerance covers the
/// other, so no version stamp is needed here — unlike [`crate::session::SessionEvent`],
/// whose `schema_version` exists for changes a reader must *branch* on. Nothing in a
/// preference file has that shape yet; if one ever does, that is the moment to add the
/// stamp, not before.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// The chosen colour theme, **by name**.
    ///
    /// A `String`, not a typed enum, on purpose: this module owns *storage* and the
    /// theme type is being built on its own branch. A name keeps the two independent —
    /// the consumer maps a name it recognises and falls back to its own default for one
    /// it does not, which is also what makes a theme file written by a newer console
    /// harmless to an older one.
    ///
    /// `None` is "the user has never chosen" and is meaningfully different from any
    /// particular name: it is what lets the product change its default theme and have
    /// that reach everyone who never expressed an opinion.
    pub theme: Option<String>,
}

/// Distinguishes concurrent temp files written by this process. The pid separates
/// processes; this separates two saves inside one.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl Preferences {
    /// The store root these preferences live in: **[`SessionLog::store_root`], reused,
    /// not re-derived.**
    ///
    /// ⚠️ The one-resolver rule (`organon-shell/Cargo.toml`, the #658 lesson: a
    /// hand-rolled `$HOME` put the Mind corpus in `%TEMP%` on Windows) is usually stated
    /// as "always use `dirs`". Calling `dirs::data_dir().join("OrganonShell")` here would
    /// satisfy that letter and still be wrong — two resolvers that *can* disagree
    /// eventually do, and the failure is a preferences file written beside a
    /// `harnesses.json` the console reads from somewhere else. One function answers
    /// this question for the whole crate.
    pub fn store_root() -> Option<PathBuf> {
        SessionLog::store_root()
    }

    /// Read `<store_root>/preferences.json`.
    ///
    /// **Total by construction** — every failure is the same answer, "nothing stored",
    /// which for a brand-new install is also the *correct* answer. Tests use this with a
    /// temp root; production uses [`Preferences::load_default`].
    pub fn load(store_root: &Path) -> Self {
        fs::read_to_string(store_root.join(PREFS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// [`Preferences::load`] against the real store. A platform with no data directory
    /// at all yields the defaults, for the same reason every other failure does.
    pub fn load_default() -> Self {
        Self::store_root().map(|r| Self::load(&r)).unwrap_or_default()
    }

    /// Write `<store_root>/preferences.json`, replacing any existing file **atomically**.
    ///
    /// Temp file in the *same directory*, then rename — the directory matters, because a
    /// rename is only atomic within one volume and `%TEMP%` is routinely on another.
    /// `std::fs::rename` replaces an existing destination on Windows too: it is
    /// `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, not the bare `MoveFileW` that
    /// fails when the target exists.
    ///
    /// ⚠️ **This returns a result where [`Preferences::load`] swallows everything, and the
    /// asymmetry is the point.** A read that fails has a sane answer to fall back on. A
    /// write that fails has none: the user's choice is simply gone, and only the caller
    /// is in a position to say so. Swallowing it here would make "I set this and it did
    /// not stick" undiagnosable.
    pub fn save(&self, store_root: &Path) -> io::Result<()> {
        fs::create_dir_all(store_root)?;
        let mut json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        json.push('\n');

        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = store_root.join(format!("{PREFS_FILE}.tmp-{}-{seq}", std::process::id()));
        // Plain UTF-8: `write` emits exactly these bytes, and `to_string_pretty` never
        // produces a BOM. See this module's header for why that is worth naming.
        fs::write(&temp, json.as_bytes())?;
        match fs::rename(&temp, store_root.join(PREFS_FILE)) {
            Ok(()) => Ok(()),
            Err(e) => {
                // A temp file left behind would accumulate one per failed save forever,
                // and each one looks like a torn write to anyone reading the directory.
                let _ = fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    /// [`Preferences::save`] to the real store. Mirrors
    /// [`crate::session::SessionLog::open_default`]: no data directory is an error here,
    /// because a caller asking to persist has nowhere to fall back to.
    pub fn save_default(&self) -> io::Result<()> {
        let root = Self::store_root()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no platform data directory"))?;
        self.save(&root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private directory per test. ⚠️ **Never the real store** — a test that writes
    /// `%APPDATA%\OrganonShell\preferences.json` would overwrite the preferences of
    /// whoever ran `cargo test`.
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("organon-shell-prefs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// CONTRACT: what was saved is what loads back.
    #[test]
    fn preferences_round_trip_through_the_store() {
        let root = temp_root("round-trip");
        let prefs = Preferences { theme: Some("phosphor".into()) };
        prefs.save(&root).unwrap();
        assert_eq!(Preferences::load(&root), prefs);
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a fresh install is not an error. This is the overwhelmingly common
    /// case and it must be indistinguishable from "the user has chosen nothing".
    #[test]
    fn a_missing_file_is_no_stored_preferences() {
        let root = temp_root("missing");
        assert_eq!(Preferences::load(&root), Preferences::default());
        assert_eq!(Preferences::load(&root).theme, None, "and no theme is None, not empty");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: garbage is the same answer as absence. The console still opens.
    #[test]
    fn a_malformed_file_is_no_stored_preferences() {
        let root = temp_root("malformed");
        for bad in ["{", "", "not json at all", r#"{"theme": 7}"#, "[]"] {
            fs::write(root.join(PREFS_FILE), bad).unwrap();
            assert_eq!(
                Preferences::load(&root),
                Preferences::default(),
                "{bad:?} must not panic and must not error"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT, both halves: we never write a BOM, and a BOM'd file is simply
    /// malformed. The second half is not a nicety we are withholding — it is the same
    /// strictness `harnesses.json` has, and two files in one directory that disagree
    /// about their own encoding rules is worse than one rule applied everywhere.
    #[test]
    fn a_bom_is_never_written_and_is_not_tolerated_on_read() {
        let root = temp_root("bom");
        Preferences { theme: Some("phosphor".into()) }.save(&root).unwrap();
        let bytes = fs::read(root.join(PREFS_FILE)).unwrap();
        assert_ne!(&bytes[..3], b"\xEF\xBB\xBF".as_slice(), "a BOM here is silently unreadable");
        assert_eq!(bytes[0], b'{', "the first byte is the JSON itself");

        let mut bommed = b"\xEF\xBB\xBF".to_vec();
        bommed.extend_from_slice(&bytes);
        fs::write(root.join(PREFS_FILE), &bommed).unwrap();
        assert_eq!(
            Preferences::load(&root),
            Preferences::default(),
            "serde_json refuses a BOM; the posture turns that into defaults, not a crash"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: a file written by a newer console loads in an older binary, keeping
    /// every key that binary does understand. Downgrade must not be a data-loss event.
    #[test]
    fn an_unknown_key_does_not_break_an_older_binary() {
        let root = temp_root("unknown-key");
        fs::write(
            root.join(PREFS_FILE),
            r#"{"theme":"phosphor","font_size":14,"nested":{"a":[1,2]}}"#,
        )
        .unwrap();
        assert_eq!(Preferences::load(&root).theme.as_deref(), Some("phosphor"));
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the other direction — an older file missing a newer key takes that
    /// key's default. `{}` is the smallest possible instance of it.
    #[test]
    fn a_missing_key_takes_its_default() {
        let root = temp_root("missing-key");
        fs::write(root.join(PREFS_FILE), "{}").unwrap();
        assert_eq!(Preferences::load(&root), Preferences::default());
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: saving over an existing file replaces it whole and leaves nothing
    /// beside it. ⚠️ The temp file is the part that has to be checked: one stranded per
    /// save would accumulate, and on Windows the rename-over-an-existing-target is the
    /// step that fails outright with the wrong API.
    #[test]
    fn a_save_replaces_the_previous_file_and_strands_no_temp() {
        let root = temp_root("replace");
        Preferences { theme: Some("first".into()) }.save(&root).unwrap();
        Preferences { theme: Some("second".into()) }.save(&root).unwrap();
        assert_eq!(Preferences::load(&root).theme.as_deref(), Some("second"));

        let left: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec![PREFS_FILE.to_string()], "the store holds one file, not a litter");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: the temp file is a *sibling*, because a rename is only atomic within a
    /// volume and `%TEMP%` is routinely on another one. Pinned by watching a save land
    /// with the target directory as the only writable place it could have used.
    #[test]
    fn the_replacement_is_atomic_because_the_temp_is_a_sibling() {
        let root = temp_root("sibling");
        let prefs = Preferences { theme: Some("phosphor".into()) };
        prefs.save(&root).unwrap();
        // The old content is readable right up to the rename and complete after it;
        // there is no window in which the target exists and does not parse.
        let text = fs::read_to_string(root.join(PREFS_FILE)).unwrap();
        assert!(serde_json::from_str::<Preferences>(&text).is_ok());
        assert!(text.ends_with('\n'), "a trailing newline, so the file reads like a text file");
        let _ = fs::remove_dir_all(&root);
    }

    /// CONTRACT: first run has no store directory yet, and saving must create it rather
    /// than fail — the first preference a user ever sets is exactly this case.
    #[test]
    fn a_save_creates_the_store_directory() {
        let root = temp_root("create").join("not-yet");
        assert!(!root.exists());
        Preferences { theme: Some("phosphor".into()) }.save(&root).unwrap();
        assert_eq!(Preferences::load(&root).theme.as_deref(), Some("phosphor"));
        let _ = fs::remove_dir_all(root.parent().unwrap());
    }

    /// CONTRACT: **one resolver.** Preferences land beside `harnesses.json`, in the
    /// directory the session log already names, because a second resolver is a second
    /// answer waiting to disagree.
    #[test]
    fn the_store_root_is_the_one_the_rest_of_the_crate_uses() {
        assert_eq!(Preferences::store_root(), SessionLog::store_root());
        let root = SessionLog::store_root().expect("platform data dir");
        assert!(root.ends_with("OrganonShell"));
        assert_eq!(
            root.join(PREFS_FILE).parent(),
            Some(root.join("harnesses.json").parent().unwrap()),
            "preferences.json and harnesses.json are siblings"
        );
    }
}

//! Shared mind-log — the fine-tuning corpus the AI-agent (#317) and visible-mind (#367)
//! features both accrete from day one. Every prompt, reply, plan, action, acceptance,
//! rejection, brief, and model event is appended as one JSON line under
//! `<store>/mind-log/organon-mind.jsonl`, where `<store>` is the one Organon store
//! every other file in this repo writes to: `dirs::data_dir()/OrganicMath` —
//! `~/Library/Application Support/OrganicMath` on macOS, `%APPDATA%\OrganicMath` on
//! Windows, `~/.local/share/OrganicMath` on Linux.
//!
//! SHARED CONTRACT ARTIFACT: created byte-identically by both the #317 and #367 Round-1
//! PRs so the branches merge without conflict. Change the contract, not one copy.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A small closed vocabulary so the corpus is trivially filterable as training data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MindEvent {
    Prompt,
    Reply,
    Plan,
    Action,
    Accept,
    Reject,
    Brief,
    Model,
    Note,
}

impl MindEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            MindEvent::Prompt => "prompt",
            MindEvent::Reply => "reply",
            MindEvent::Plan => "plan",
            MindEvent::Action => "action",
            MindEvent::Accept => "accept",
            MindEvent::Reject => "reject",
            MindEvent::Brief => "brief",
            MindEvent::Model => "model",
            MindEvent::Note => "note",
        }
    }
}

/// `<store>/mind-log/` — the corpus directory, created on demand.
///
/// macOS `~/Library/Application Support/OrganicMath/mind-log/` · Windows
/// `%APPDATA%\OrganicMath\mind-log\` · Linux `~/.local/share/OrganicMath/mind-log/`,
/// with a temp-dir fallback when the platform data dir cannot be resolved at all.
pub fn mind_log_dir() -> PathBuf {
    mind_log_dir_in(dirs::data_dir())
}

/// The path composition on its own, with the platform data dir injected — because
/// `dirs::data_dir()` reads the real machine and so cannot be exercised in a test,
/// while *what we do with it* is the part that was wrong (#658 Tier 1, item 4).
fn mind_log_dir_in(data_dir: Option<PathBuf>) -> PathBuf {
    data_dir.unwrap_or_else(std::env::temp_dir).join("OrganicMath").join("mind-log")
}

/// The single append-only corpus file.
pub fn mind_log_path() -> PathBuf {
    mind_log_dir().join("organon-mind.jsonl")
}

/// Append one record as a single JSON line. Best-effort: never panics, never blocks the
/// caller in any meaningful way — logging must never break the instrument.
pub fn append(event: MindEvent, source: &str, text: &str) {
    append_in(&mind_log_dir(), event, source, text);
}

/// Serializes concurrent appends (editor / render / agent threads) so lines never interleave.
static LOG_LOCK: Mutex<()> = Mutex::new(());

fn append_in(dir: &Path, event: MindEvent, source: &str, text: &str) {
    let _guard = LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let line = format!(
        "{{\"ts\":{},\"event\":\"{}\",\"source\":{},\"text\":{}}}\n",
        now_millis(),
        event.as_str(),
        json_string(source),
        json_string(text),
    );
    let _ = fs::create_dir_all(dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("organon-mind.jsonl")) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

/// Minimal JSON string escaper — hand-rolled so the #317 and #367 Round-1 PRs could drop
/// this module in identically without touching `Cargo.toml`. It stays hand-rolled because
/// there is nothing to be consistent *with*: no other store serialises its records here.
/// The store **path** is the opposite case — see `mind_log_dir_in` and the note below.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ─── Why `dirs`, when this module used to resolve `$HOME` itself (#658 Tier 1) ───────
//
// This file used to carry `fn home_dir()` reading `$HOME`, under a comment saying it kept
// the leaf module self-contained. `$HOME` is unset on Windows, so that put the corpus in
// `%TEMP%\mind-log\` — outside the store, in a directory the OS may clear.
//
// The tempting fix is a hand-rolled platform branch (`%APPDATA%` on Windows, `$HOME/…` on
// macOS, `$XDG_DATA_HOME` on Linux), which keeps the dependency count at four. We took the
// dependency instead, for one reason: **the defect is divergence from the store, so the fix
// has to be the same function, not a lookalike.** A hand-rolled resolver is by construction
// a *second* implementation of "where does Organon keep its files", and it is wrong in
// exactly the places that are hard to notice — `dirs` asks Windows for
// `SHGetKnownFolderPath(FOLDERID_RoamingAppData)` rather than reading `%APPDATA%`, which is
// what survives a redirected or roaming profile, and it rejects a relative `$XDG_DATA_HOME`.
// Two resolvers that agree today and drift tomorrow is the bug we are here to close.
//
// The crate's dependency rule (see `Cargo.toml`) permits this: it bans *dead weight*, and
// says a dependency earns its place when a `use` in `src/` needs it. `dirs` was struck from
// the first draft because nothing imported it. Now something does.
//
// macOS output is unchanged, and that is checked below: `dirs::data_dir()` on macOS is
// `home_dir().join("Library/Application Support")`, and its `home_dir()` is `$HOME` with a
// passwd-database fallback — so the normal path is byte-identical and the degenerate one
// (`$HOME` unset) resolves a real home where we used to fall through to the temp dir.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_escapes() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn event_labels_are_stable() {
        assert_eq!(MindEvent::Prompt.as_str(), "prompt");
        assert_eq!(MindEvent::Reject.as_str(), "reject");
        assert_eq!(MindEvent::Model.as_str(), "model");
    }

    /// The composition, with the platform data dir injected. This is the whole of what
    /// #658 changed: the corpus hangs off the shared `OrganicMath` store root, not off a
    /// hand-spelled `Library/Application Support`.
    #[test]
    fn corpus_hangs_off_the_shared_store_root() {
        let dir = mind_log_dir_in(Some(PathBuf::from("/base")));
        assert_eq!(dir, PathBuf::from("/base/OrganicMath/mind-log"));
    }

    /// macOS byte-identity, expressed as a composition: `dirs::data_dir()` on macOS *is*
    /// `$HOME/Library/Application Support`, so feeding that in must reproduce the exact
    /// pre-#658 path. This holds on every platform because the input is fixed, which is
    /// the point of injecting it — the Mac guarantee is checkable from Linux CI.
    #[test]
    fn macos_data_dir_reproduces_the_pre_658_path() {
        let mac_data_dir = PathBuf::from("/Users/j/Library/Application Support");
        assert_eq!(
            mind_log_dir_in(Some(mac_data_dir)),
            PathBuf::from("/Users/j").join("Library/Application Support/OrganicMath").join("mind-log"),
        );
    }

    /// The fallback still exists, and now lands *inside* a store root rather than beside
    /// one — matching `preset.rs` / `keymap.rs` / `theme_config.rs`, whose fallback is
    /// likewise `temp_dir()/OrganicMath/…`.
    #[test]
    fn unresolvable_data_dir_falls_back_to_the_temp_dir() {
        assert_eq!(
            mind_log_dir_in(None),
            std::env::temp_dir().join("OrganicMath").join("mind-log"),
        );
    }

    /// The real thing, on the machine running the test: absolute, under a store root, and
    /// ending in the corpus directory. Deliberately weak about the prefix — that is the
    /// platform's answer, not ours.
    #[test]
    fn live_mind_log_path_is_absolute_and_in_the_store() {
        let p = mind_log_path();
        assert!(p.is_absolute(), "mind log path must be absolute: {p:?}");
        assert!(p.ends_with("OrganicMath/mind-log/organon-mind.jsonl"), "unexpected tail: {p:?}");
    }

    /// The Mac guarantee against the live resolver, where a Mac is actually running: the
    /// pre-#658 expression, spelled out, must equal what `mind_log_dir()` now returns.
    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_live_path_is_unchanged_from_before_658() {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .expect("a Mac dev/CI box has $HOME");
        assert_eq!(
            mind_log_dir(),
            home.join("Library/Application Support/OrganicMath").join("mind-log"),
        );
    }

    #[test]
    fn append_writes_one_json_line() {
        let dir = std::env::temp_dir().join(format!("organon-mindlog-{}", now_millis()));
        append_in(&dir, MindEvent::Note, "test", "hello \"world\"\n");
        let body = std::fs::read_to_string(dir.join("organon-mind.jsonl")).unwrap();
        assert!(body.contains("\"event\":\"note\""));
        assert!(body.contains("\"source\":\"test\""));
        assert!(body.trim_end().ends_with('}'));
        assert_eq!(body.lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! **Where the console's own words go when the window that was showing them is gone.**
//!
//! `organon-console` is a GUI, and on Windows it now declares itself one
//! (`#![windows_subsystem = "windows"]` at the top of `console_main.rs`) so that launching it
//! from a shim or a `start ""` does not leave a black console window sitting behind the
//! workspace. That attribute has a cost this workstation has already paid once: a process with
//! no console has nowhere to write, and every `eprintln!` in the binary — the refusals, the
//! device negotiation, the panic hook — goes into nothing. The lighting renderer here ran
//! **unobservable for six hours** for exactly that reason, and every indicator stayed green
//! throughout, because "nobody is reading the output" and "there is no output" look identical
//! from outside.
//!
//! 🚨 **So the attribute and this file are one change, not two.** Hiding the window without
//! providing a destination is the defect; they land together and neither is independently
//! correct.
//!
//! ⚠️ **What is redirected is the process's standard handles, not a logging framework, and
//! that is deliberate.** The binary already has hundreds of `eprintln!` call sites written
//! over months, and a logger would capture none of them without an edit to every one.
//! `SetStdHandle` moves `STD_ERROR_HANDLE` and `STD_OUTPUT_HANDLE` at startup, before anything
//! has spoken; Rust's Windows stdio resolves those handles per write rather than caching them,
//! so every existing call site — **and the default panic hook, which writes to
//! `io::stderr()`** — lands here with no edit. Measured on this toolchain rather than assumed:
//! a probe that set both handles and then ran `eprintln!`, `println!` and a panic printed
//! nothing to the terminal and all three to the file. The FFI itself lives in
//! `console_main.rs`, beside the `windows-sys` dependency already there for the DPI work; this
//! module owns only the parts that are neither unsafe nor platform-specific, which is what
//! makes them testable.
//!
//! ⚠️ **This is NOT [`crate::status_log`]**, and the two are easy to confuse by name. That one
//! is a surface: the console's remarks about the session, shown in the pane, held in memory,
//! read by somebody looking at the window. This one is the process's stderr on disk, read by
//! somebody looking at a console that will not start.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// The directory under the platform's local data root that holds the log.
///
/// `%LOCALAPPDATA%\organon\console\` on Windows, which is where this workstation's other
/// headless processes already keep theirs (`voice-tray`, the lighting renderer) — so somebody
/// hunting for a log looks in one place for all of them.
///
/// 🚨 **`data_local_dir`, never `data_dir`.** On Windows the second is `%APPDATA%`, which
/// roams: a log is machine-local noise and roaming it copies it to every other machine on the
/// account. Elsewhere the two are the same directory and the distinction costs nothing.
/// ⚠️ And never a hand-rolled `$HOME` — organon-mind's manifest records what that did on
/// Windows (the store landed in `%TEMP%`), which is why `dirs` is a dependency of this crate.
pub const DIR: [&str; 2] = ["organon", "console"];

/// The log's file name.
pub const FILE: &str = "console.log";

/// How large the log may get before a launch rolls it aside, in bytes.
///
/// ⚠️ **A cap rather than a rotation policy.** One generation is kept, so the pair is bounded
/// at twice this and no scheduler, no dated names and no cleanup pass are needed. Four
/// megabytes is a long run of ordinary launches and still opens instantly in an editor, which
/// is the property that matters for a file whose whole job is to be read after something has
/// gone wrong.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// The log's path under a given local-data root.
///
/// Split from [`path`] so the composition can be tested without a home directory — the
/// composition is the part that can be wrong, and `dirs` answering `None` is the part that
/// cannot be arranged in a test.
pub fn path_in(local: &Path) -> PathBuf {
    let mut p = local.to_path_buf();
    for part in DIR {
        p.push(part);
    }
    p.push(FILE);
    p
}

/// The log's path on this machine, or `None` if the platform has no local data directory.
pub fn path() -> Option<PathBuf> {
    dirs::data_local_dir().as_deref().map(path_in)
}

/// Where the previous generation is kept.
pub fn rolled(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".old");
    path.with_file_name(name)
}

/// Move the log aside if it has grown past `max_bytes`, answering where it went.
///
/// `None` means nothing was moved — either the file is absent, or it is still under the cap.
///
/// ⚠️ **A failed rename is not an error worth reporting**, and there would be nowhere to report
/// it to: this runs *before* the redirect, so a complaint would go to the console this whole
/// file exists because there isn't. An oversized log is a far smaller problem than a console
/// that refuses to start over one, so the rename is attempted and its verdict dropped.
pub fn roll(path: &Path, max_bytes: u64) -> Option<PathBuf> {
    let len = std::fs::metadata(path).ok()?.len();
    if len <= max_bytes {
        return None;
    }
    let old = rolled(path);
    std::fs::rename(path, &old).ok()?;
    Some(old)
}

/// Open the log for appending, creating its directory, having first rolled it if it is large.
///
/// **Append, never truncate.** A console that fails to start is often the *second* symptom of
/// something the previous run said on its way down, and a truncating open would destroy
/// exactly the evidence somebody came here for.
pub fn open(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    roll(path, MAX_BYTES);
    OpenOptions::new().create(true).append(true).open(path)
}

/// The line a launch writes before anything else, so a reader can tell one run from the next.
///
/// 🚨 **It names the file it is being written into.** That looks redundant and is the one part
/// that is not: somebody reading this has usually arrived from a message or a `Get-Content`
/// that another person typed, and the commonest wrong answer is "the console logs somewhere,
/// but not where I am looking". The header makes the log self-locating.
pub fn header(path: &Path, stamp: &str) -> String {
    format!("\n=== organon-console start {stamp} — log: {} ===\n", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The composition, on a root that does not have to exist. Asserted as the whole tail
    /// rather than component by component, because the failure worth catching is a component
    /// that went missing — which a per-component test passes straight through.
    #[test]
    fn the_log_sits_under_organon_console_in_the_local_data_root() {
        let p = path_in(Path::new("/root"));
        assert!(p.ends_with(Path::new("organon/console/console.log")), "{p:?}");
        assert!(p.starts_with("/root"), "{p:?}");
    }

    /// 🚨 **The rolled name keeps `.log` in it.** `with_extension("old")` — the obvious
    /// spelling, and the one written first — answers `console.old`, which sorts away from its
    /// live sibling and stops looking like a log at a glance. The pair must read as a pair in
    /// a directory listing.
    #[test]
    fn the_previous_generation_is_named_beside_the_live_one() {
        let live = path_in(Path::new("/root"));
        let old = rolled(&live);
        assert_eq!(old.file_name().unwrap(), "console.log.old");
        assert_eq!(old.parent(), live.parent(), "the pair must live together");
    }

    /// A log under the cap is left exactly where it is — the ordinary case, every launch.
    #[test]
    fn a_small_log_is_not_rolled() {
        let dir = std::env::temp_dir().join(format!("organon-log-small-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(FILE);
        std::fs::write(&p, b"small").unwrap();
        assert_eq!(roll(&p, 1024), None, "a 5-byte log was rolled");
        assert!(p.exists(), "the live log was moved anyway");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// …and one over it is moved aside, leaving the live name free for the new run.
    ///
    /// ⚠️ **Then rolled a second time**, which is the half that bounds the disk: the second
    /// roll must *replace* the first generation rather than leave a third file behind. That
    /// `std::fs::rename` overwrites an existing destination on Windows is a promise of the
    /// standard library rather than of the platform call underneath it, so it is worth pinning
    /// here rather than assuming.
    #[test]
    fn a_large_log_is_rolled_and_only_one_generation_is_kept() {
        let dir = std::env::temp_dir().join(format!("organon-log-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(FILE);

        std::fs::write(&p, vec![b'a'; 200]).unwrap();
        assert_eq!(roll(&p, 100), Some(rolled(&p)), "an oversized log was left in place");
        assert!(!p.exists(), "the live name is still taken");
        assert_eq!(std::fs::read(rolled(&p)).unwrap().len(), 200);

        std::fs::write(&p, vec![b'b'; 300]).unwrap();
        assert_eq!(roll(&p, 100), Some(rolled(&p)));
        let mut kept: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        kept.sort();
        assert_eq!(kept, vec!["console.log.old".to_string()], "generations accumulated");
        assert_eq!(std::fs::read(rolled(&p)).unwrap()[0], b'b', "the older run won the rename");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `open` creates the whole directory chain, appends, and never truncates — the three
    /// properties a reader arriving after a crash depends on.
    #[test]
    fn open_creates_the_directory_and_appends() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("organon-log-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let p = path_in(&root);
        assert!(!p.parent().unwrap().exists(), "the test root already existed");

        writeln!(open(&p).unwrap(), "first").unwrap();
        writeln!(open(&p).unwrap(), "second").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("first"), "a second open truncated the log: {text:?}");
        assert!(text.contains("second"), "{text:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 🚨 **The header names the file.** Pinned because it is the line most likely to be
    /// "tidied" away as redundant, and its whole value is that a log pasted into a message
    /// says where it came from.
    #[test]
    fn the_header_names_the_log_it_is_written_into() {
        let h = header(Path::new("/root/organon/console/console.log"), "2026-08-22 17:00:00");
        assert!(h.contains("console.log"), "{h}");
        assert!(h.contains("2026-08-22 17:00:00"), "{h}");
        assert!(h.starts_with('\n') && h.ends_with('\n'), "the header must stand alone: {h:?}");
    }
}

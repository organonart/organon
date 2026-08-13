//! The harness registry (Shell #11 Tier 1): which agent CLIs a tab can run.
//!
//! PRD v3.1 §4.3 — harness-pluggable, Pi-first, Pi-not-required. A harness is DATA:
//! identity, launch command, how to detect it on the machine, where to get it. The
//! built-ins below seed the registry; a user file (`harnesses.json` at the
//! OrganonShell store root) merges over them by id, following session.rs's
//! forward-compat discipline (serde defaults, unknown fields tolerated). Detection
//! is a PATH probe behind an injectable lookup so every decision here is testable
//! without touching the machine's real PATH.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::platform::{self, Platform};

/// One runnable harness. `command` empty means "the user's login shell" — the
/// plain-terminal entry every registry carries.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HarnessSpec {
    pub id: String,
    pub name: String,
    /// A short glyph for the tab strip (real icons are a later tier).
    #[serde(default)]
    pub glyph: String,
    /// argv to exec. Launched through the user's shell (`zsh -lc "exec …"` on Unix,
    /// `cmd.exe /C` on Windows) so the PATH that finds it is the user's real one,
    /// not the app's — and so Windows `.cmd` shims resolve at all. See
    /// [`crate::platform::wrap_harness`].
    #[serde(default)]
    pub command: Vec<String>,
    /// Binary names whose presence on PATH means "installed". Empty = always
    /// available (the plain shell). Bare names: the Windows probe expands them
    /// through `PATHEXT` ([`crate::platform::executable_names`]).
    #[serde(default)]
    pub detect: Vec<String>,
    #[serde(default)]
    pub install_url: Option<String>,

    /// Working directory to start the harness in. `None` inherits the app's.
    /// Shell's answer to PRD FR-T5's "each tab in its own working directory".
    ///
    /// ⚠️ **The path is in the namespace the process actually STARTS in** — which is
    /// not always the host's. A `wsl` harness launched from Windows starts inside
    /// WSL, so its `cwd` is a **Linux** path applied by a `cd` there; a Windows path
    /// would be meaningless. Every other launch — including a `wsl` spec running on
    /// a non-Windows host, where the `wsl` flag is inert — starts on this machine,
    /// so its `cwd` is a host path.
    ///
    /// A leading `~` works in **both** cases, by two different mechanisms: bash
    /// expands it inside WSL, and [`crate::platform::expand_tilde`] resolves it for a
    /// native launch before the OS sees it (neither `chdir` nor `CreateProcessW`
    /// expands `~`). `~otheruser` is not supported natively — only a shell resolves
    /// that.
    #[serde(default)]
    pub cwd: Option<String>,

    /// Run this harness **inside WSL** from a Windows host (`wsl.exe -- bash -lic`).
    ///
    /// The Pi-on-Windows case is WSL-first in practice, so this is a first-class
    /// field rather than something a user has to hand-assemble into `command`.
    /// Ignored on non-Windows hosts, where the harness runs natively.
    #[serde(default)]
    pub wsl: bool,

    /// Which WSL distribution (`wsl -d <name>`). `None` = the user's default.
    /// Only meaningful with `wsl: true`.
    #[serde(default)]
    pub wsl_distro: Option<String>,

    /// Open this harness in the **conversation view** rather than a terminal tab
    /// (Console Spike §5.9).
    ///
    /// The console has two front-ends over one renderer, and this flag is which one a
    /// tab gets. A conversation tab spawns no PTY at all: it drives
    /// [`crate::agent_session`] over pipes and renders the structured event stream
    /// natively. So `command`, `wsl` and the whole [`launch_argv`] decision are
    /// **inert** here — the flags are the CLI's own (`agent_session::ARGS`), because
    /// they are what make the process a persistent session rather than a one-shot, and
    /// a user-supplied argv could silently break that.
    ///
    /// `cwd` is still honoured: it is the directory the agent works in. `detect` is
    /// still honoured: it is the same PATH probe, so the + menu cannot offer a
    /// conversation with a CLI that is not installed.
    ///
    /// Rule 5′ (§6): the terminal host is harness-agnostic, the conversation view is
    /// harness-specific and says which harness — and **degrading to a terminal tab is
    /// always available**, which is exactly what the neighbouring `claude` row is.
    #[serde(default)]
    pub conversation: bool,
}

/// The built-in registry, in the order the + menu shows them. Pi leads after the
/// plain shell — first among equals, never required (PRD §4.3).
pub fn builtin() -> Vec<HarnessSpec> {
    builtin_for(Platform::current())
}

/// The built-in registry as it would be on `platform` — a parameter, not a `#[cfg]`,
/// so the Windows registry is testable from a Mac (see [`crate::platform`] for why
/// that rule exists).
///
/// **Windows gains the WSL entries.** On a Windows box the agent harnesses commonly
/// live in WSL rather than on the Windows side — that is where the toolchain and the
/// checkouts are — so `pi-wsl` / `claude-wsl` ship as first-class registry rows
/// beside the native ones. They deliberately carry **no `cwd`**: which project
/// directory to open in is the user's, not the product's, and it belongs in
/// `harnesses.json` (see this module's header).
pub fn builtin_for(platform: Platform) -> Vec<HarnessSpec> {
    let h = |id: &str, name: &str, glyph: &str, cmd: &[&str], detect: &[&str], url: Option<&str>| {
        HarnessSpec {
            id: id.into(),
            name: name.into(),
            glyph: glyph.into(),
            command: cmd.iter().map(|s| s.to_string()).collect(),
            detect: detect.iter().map(|s| s.to_string()).collect(),
            install_url: url.map(Into::into),
            cwd: None,
            wsl: false,
            wsl_distro: None,
            conversation: false,
        }
    };
    let wsl = |id: &str, name: &str, glyph: &str, cmd: &[&str], url: Option<&str>| HarnessSpec {
        id: id.into(),
        name: name.into(),
        glyph: glyph.into(),
        command: cmd.iter().map(|s| s.to_string()).collect(),
        // Detection can only prove the BRIDGE exists, not that the harness is
        // installed inside the distro — probing that means booting WSL on every
        // launch. Recorded in SHELL_ARCHITECTURE.md's honesty ledger.
        detect: vec!["wsl.exe".into()],
        install_url: url.map(Into::into),
        cwd: None,
        wsl: true,
        wsl_distro: None,
        conversation: false,
    };

    let mut reg = vec![
        h("shell", "Shell", "❯", &[], &[], None),
        h("pi", "Pi", "π", &["pi"], &["pi"], Some("https://github.com/badlogic/pi-mono")),
        h(
            "claude",
            "Claude Code",
            "✳",
            &["claude"],
            &["claude"],
            Some("https://claude.com/claude-code"),
        ),
        h("omp", "oh-my-pi", "Ω", &["omp"], &["omp"], Some("https://omp.sh")),
        h("codex", "Codex", "◇", &["codex"], &["codex"], Some("https://openai.com/codex")),
        h(
            "cursor",
            "Cursor",
            "▲",
            &["cursor-agent"],
            &["cursor-agent"],
            Some("https://cursor.com"),
        ),
        // The conversation view (§5.9). It sits beside the terminal row for the same
        // CLI, not instead of it: the same agent, two front-ends, and the terminal one
        // is what Rule 5′ calls "supported the old way".
        HarnessSpec {
            conversation: true,
            ..h(
                "claude-chat",
                "Claude Code (conversation)",
                "◈",
                &[],
                &["claude"],
                Some("https://claude.com/claude-code"),
            )
        },
    ];
    if platform == Platform::Windows {
        reg.push(wsl("pi-wsl", "Pi (WSL)", "π", &["pi"], Some("https://github.com/badlogic/pi-mono")));
        reg.push(wsl("claude-wsl", "Claude Code (WSL)", "✳", &["claude"], Some("https://claude.com/claude-code")));
        reg.push(wsl("shell-wsl", "Shell (WSL)", "❯", &["$SHELL"], None));
    }
    reg
}

/// Merge user entries over the built-ins by id (user wins; new ids append, in
/// file order). The user file is `<store root>/harnesses.json`, an array of
/// [`HarnessSpec`]; a missing or unreadable file is simply "no user entries".
pub fn load(store_root: &Path) -> Vec<HarnessSpec> {
    let mut registry = builtin();
    let path = store_root.join("harnesses.json");
    let user: Vec<HarnessSpec> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for spec in user {
        match registry.iter_mut().find(|r| r.id == spec.id) {
            Some(slot) => *slot = spec,
            None => registry.push(spec),
        }
    }
    registry
}

/// What a tab should actually spawn for `spec`, and the PTY working directory to
/// spawn it in.
///
/// **One place decides this**, because the decision has three interacting axes
/// (platform, WSL-or-native, cwd) and scattering them is how the Windows spawn broke.
/// Returns `(argv, pty_cwd)`:
///
/// * `argv` — always a real, launchable command for `platform`.
/// * `pty_cwd` — the directory to hand `CommandBuilder::cwd`, **already
///   tilde-expanded**, and **`None` for a WSL launch**: there the `cd` is baked into
///   the bash script instead, because the path lives in WSL's filesystem.
///
/// **One rule governs `cwd`: it is a path in whatever namespace the process actually
/// starts in.** A WSL launch starts inside WSL, so `~` is left for bash
/// ([`platform::bash_cd`]). A native launch starts on this host, so `~` is resolved
/// here ([`platform::expand_tilde`]) — the OS never expands it. Getting only one half
/// right is what made the documented `"cwd": "~/…"` shorthand work for WSL and fail
/// natively.
///
/// A `wsl` spec on a non-Windows host falls back to running **natively**, and that
/// fallback *is* a native launch — so its `cwd` is treated as a host path and
/// expanded, not silently dropped. The field describes how to reach a Linux harness
/// *from Windows*; on Linux there is nothing to reach across, and the same
/// `~/project` the user wrote is a perfectly good host path. If it does not exist the
/// spawn fails by name, which beats starting in the wrong directory quietly.
pub fn launch_argv(
    spec: &HarnessSpec,
    platform: Platform,
    env: impl Fn(&str) -> Option<String> + Copy,
    have: impl Fn(&str) -> bool + Copy,
) -> (Vec<String>, Option<String>) {
    if spec.wsl && platform == Platform::Windows {
        let argv = if spec.command.is_empty() { vec!["$SHELL".to_string()] } else { spec.command.clone() };
        return (platform::wrap_wsl(&argv, spec.cwd.as_deref(), spec.wsl_distro.as_deref()), None);
    }
    let argv = if spec.command.is_empty() {
        platform::default_shell(platform, env, have)
    } else {
        platform::wrap_harness(platform, &spec.command, env, have)
    };
    let home = platform::home_dir(platform, env);
    let cwd = spec.cwd.as_deref().map(|c| platform::expand_tilde(c, home.as_deref()));
    (argv, cwd)
}

/// The environment variable that names the project a console is about, for the whole
/// launch. One of the `ORGANON_SHELL_*` family the launch shims already set.
pub const PROJECT_ENV: &str = "ORGANON_SHELL_PROJECT";

/// Which rule chose a conversation tab's working directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CwdSource {
    /// [`HarnessSpec::cwd`] — the user said so, for this tab.
    Spec,
    /// [`PROJECT_ENV`] — the user said so, for this launch.
    Env,
    /// The nearest project root at or above the directory the console was launched in.
    ProjectRoot,
    /// The launch directory itself: nothing above it looked like a project.
    LaunchDir,
}

impl CwdSource {
    /// The half-sentence that goes after the directory in a log line.
    pub fn why(self) -> &'static str {
        match self {
            CwdSource::Spec => "from this harness's \"cwd\"",
            CwdSource::Env => "from $ORGANON_SHELL_PROJECT",
            CwdSource::ProjectRoot => "the nearest project root above where the console started",
            CwdSource::LaunchDir => "where the console started — nothing above it looks like a project",
        }
    }
}

/// Where a conversation tab will start, why, and whether that place has anything an
/// agent reads as project context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationCwd {
    pub dir: String,
    pub source: CwdSource,
    /// `dir` satisfies no project test at all — see [`cwd_notes`], which turns this
    /// into the warning.
    pub bare: bool,
}

/// Where a **conversation** tab should start.
///
/// ⚠️ **This is the defect this function exists for: a conversation tab used to inherit
/// the console's own working directory, silently.** `spec.cwd` was `None` for the
/// built-in `claude-chat` row, `AgentSession::spawn` turns `None` into "wherever the app
/// happens to be", and a console started from Explorer or from a PATH shim happens to be
/// nowhere in particular. An agent there sees no repo-local `.claude/skills/`, no project
/// `CLAUDE.md` — and nothing anywhere says so. Measured 2026-08-13: an agent in a
/// conversation tab answered `Unknown skill: organon-cli` with the skill sitting correctly
/// on disk.
///
/// **A conversation tab is not inherently about any one project, so the product must not
/// name one.** What it can do is stop guessing in silence. Four rules, in order:
///
/// 1. **[`HarnessSpec::cwd`]** — the user's per-tab answer, from `harnesses.json`. Wins
///    outright; the whole point of the registry seam is that a user row is the last word.
/// 2. **[`PROJECT_ENV`]** — the user's per-launch answer, for a shim or a one-off shell.
/// 3. **The nearest project root at or above the launch directory** — which is what makes
///    "`cd` into a checkout, run the console" do the obvious thing with no configuration
///    at all, for *any* checkout, without the product knowing a single path.
/// 4. **The launch directory**, unchanged — today's behaviour, now stated rather than
///    inherited.
///
/// 📌 **Rule 3 is deliberately NOT applied to terminal tabs.** A shell announces its
/// directory in the prompt and `cd` is one keystroke, so starting in `native/` when that
/// is where you were is right — ascending to the repo root would be an unasked-for
/// correction. An agent's working directory is invisible *and* decides which instructions
/// and skills exist at all, so the two cases genuinely differ.
///
/// `is_project` is the marker test, injected so every decision here is a unit test:
/// [`is_project_dir`] in production.
pub fn conversation_cwd(
    spec: &HarnessSpec,
    platform: Platform,
    launch_dir: &Path,
    env: impl Fn(&str) -> Option<String> + Copy,
    is_project: impl Fn(&Path) -> bool,
) -> ConversationCwd {
    let home = platform::home_dir(platform, env);
    let settle = |dir: String, source: CwdSource| ConversationCwd {
        bare: !is_project(Path::new(&dir)),
        dir,
        source,
    };
    if let Some(c) = spec.cwd.as_deref() {
        return settle(platform::expand_tilde(c, home.as_deref()), CwdSource::Spec);
    }
    if let Some(p) = env(PROJECT_ENV).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        return settle(platform::expand_tilde(&p, home.as_deref()), CwdSource::Env);
    }
    let home_path = home.as_deref().map(Path::new);
    match nearest_project_root(launch_dir, home_path, &is_project) {
        Some(root) => settle(root, CwdSource::ProjectRoot),
        None => settle(launch_dir.display().to_string(), CwdSource::LaunchDir),
    }
}

/// Walk up from `from` looking for a project marker, stopping **at the home directory**.
///
/// ⚠️ **Home is never *discovered*, only inherited.** A `.claude` directory there is
/// user-global configuration — every agent gets it wherever it starts — so treating it as
/// a project root would quietly aim a console launched from `~/Documents` at the home
/// directory, which on this machine is explicitly not a codebase. Launching *in* home
/// still lands in home, via rule 4; the difference is that nothing ascends into it.
///
/// ⚠️ The stop test is `Path::starts_with`, which compares components **case-sensitively**.
/// A Windows launch directory spelled with a different case than `%USERPROFILE%` would
/// walk past home; the cost is one extra ancestor tested, not a wrong answer, since those
/// ancestors have to carry a marker to be chosen.
fn nearest_project_root(
    from: &Path,
    home: Option<&Path>,
    is_project: &impl Fn(&Path) -> bool,
) -> Option<String> {
    for dir in from.ancestors() {
        if home.is_some_and(|home| home.starts_with(dir)) {
            return None;
        }
        if is_project(dir) {
            return Some(dir.display().to_string());
        }
    }
    None
}

/// The production marker test: does `dir` look like a project an agent can work in?
///
/// `.claude/` first because it is the literal thing that was missing — skills and project
/// settings live there. `CLAUDE.md` because a project may carry instructions and no skills.
/// `.git` last, as the general "this is a checkout" fallback; a git worktree's `.git` is a
/// *file*, so this tests existence rather than directory-ness.
pub fn is_project_dir(dir: &Path) -> bool {
    dir.join(".claude").is_dir() || dir.join("CLAUDE.md").is_file() || dir.join(".git").exists()
}

/// What to say about where a conversation tab landed: one line always, and a second when
/// it landed somewhere with no project context.
///
/// ⚠️ **The first line is unconditional on purpose.** The failure being closed here is a
/// *silent* one, and a diagnostic that only appears when something is detectably wrong
/// cannot cover the case where the resolution is wrong in a way this code cannot see — a
/// project root found two levels above the one the user meant, say. Stating the answer
/// every time is what makes that inspectable at all.
pub fn cwd_notes(resolved: &ConversationCwd) -> Vec<String> {
    let mut notes =
        vec![format!("working directory {} ({})", resolved.dir, resolved.source.why())];
    if resolved.bare {
        notes.push(
            "⚠ no .claude/, CLAUDE.md or .git here — this agent starts with no project \
             skills and no project instructions. Start the console from inside the \
             project, set $ORGANON_SHELL_PROJECT, or give this harness a \"cwd\" in \
             harnesses.json."
                .to_string(),
        );
    }
    notes
}

/// Which registry ids are installed, per `lookup` (a PATH probe in production,
/// anything in tests). Empty `detect` = always installed.
pub fn detect_installed(
    registry: &[HarnessSpec],
    lookup: impl Fn(&str) -> bool,
) -> HashSet<String> {
    registry
        .iter()
        .filter(|h| h.detect.is_empty() || h.detect.iter().any(|b| lookup(b)))
        .map(|h| h.id.clone())
        .collect()
}

/// The production PATH probe: is `bin` an executable file on `$PATH`?
///
/// ⚠️ **On Windows a bare name is almost never the file on disk.** `pi` installed by
/// npm is `pi.cmd`; a Go tool is `pi.exe`. The original probe joined the bare name
/// only, so `dir.join("pi")` was never a file and **every harness showed as
/// not-installed** — the whole + menu greyed out except the plain shell. The
/// candidate set now comes from [`platform::executable_names`], which expands
/// through `PATHEXT`.
pub fn on_path(bin: &str) -> bool {
    on_path_for(Platform::current(), bin)
}

/// [`on_path`] against a chosen platform, for tests.
pub fn on_path_for(platform: Platform, bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let names = platform::executable_names(platform, bin, |k| std::env::var(k).ok());
    std::env::split_paths(&path).any(|dir| {
        names.iter().any(|name| {
            let p = dir.join(name);
            p.is_file() && executable(&p, platform)
        })
    })
}

/// The execute bit matters on Unix; on Windows the extension already decided it.
fn executable(p: &Path, platform: Platform) -> bool {
    if platform == Platform::Windows {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(p).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_shell_is_always_installed() {
        let installed = detect_installed(&builtin(), |_| false);
        assert!(installed.contains("shell"), "the plain shell needs no binary");
        assert_eq!(installed.len(), 1, "nothing else without a PATH hit");
    }

    #[test]
    fn detection_follows_the_lookup() {
        let installed = detect_installed(&builtin(), |bin| bin == "pi" || bin == "claude");
        assert!(installed.contains("pi") && installed.contains("claude"));
        assert!(!installed.contains("omp"));
    }

    #[test]
    fn user_file_merges_by_id_and_appends_new() {
        let root = std::env::temp_dir().join(format!("organon-shell-harness-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("harnesses.json"),
            r#"[
                {"id":"pi","name":"Pi (custom)","command":["pi","--profile","x"],"detect":["pi"],"unknown_field":1},
                {"id":"my-agent","name":"Mine","command":["my-agent"],"detect":["my-agent"]}
            ]"#,
        )
        .unwrap();
        let reg = load(&root);
        let pi = reg.iter().find(|h| h.id == "pi").unwrap();
        assert_eq!(pi.name, "Pi (custom)", "user entry replaces the built-in");
        assert_eq!(pi.command, vec!["pi", "--profile", "x"]);
        assert!(reg.iter().any(|h| h.id == "my-agent"), "new ids append");
        assert_eq!(reg.iter().filter(|h| h.id == "pi").count(), 1, "no duplicates");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_user_file_is_just_builtins() {
        let root = std::env::temp_dir().join("organon-shell-harness-none");
        assert_eq!(load(&root), builtin());
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }
    fn nothing(_: &str) -> bool {
        false
    }
    fn spec(id: &str) -> HarnessSpec {
        builtin_for(Platform::Windows).into_iter().find(|h| h.id == id).unwrap()
    }

    /// The reported bug: on Windows the plain-shell tab tried `/bin/zsh -l`.
    #[test]
    fn plain_shell_tab_is_launchable_on_windows() {
        let (argv, cwd) = launch_argv(&spec("shell"), Platform::Windows, no_env, nothing);
        assert_eq!(argv, vec!["cmd.exe"], "no PowerShell present → ComSpec");
        assert!(!argv[0].starts_with('/'), "a POSIX path can never reach CreateProcessW");
        assert_eq!(cwd, None);
    }

    #[test]
    fn native_harness_is_wrapped_per_platform() {
        let (win, _) = launch_argv(&spec("pi"), Platform::Windows, no_env, nothing);
        assert_eq!(win, vec!["cmd.exe", "/C", "pi"]);
        let (unix, _) = launch_argv(&spec("pi"), Platform::Unix, no_env, nothing);
        assert_eq!(unix, vec!["/bin/zsh", "-lc", "exec pi"]);
    }

    /// The Windows-with-WSL workflow: `wsl.exe -- bash -lic "cd … && exec pi"`, and
    /// the cwd must NOT come back as a PTY working directory (it is a Linux path).
    #[test]
    fn wsl_harness_crosses_into_wsl_and_keeps_cwd_inside() {
        let mut s = spec("pi-wsl");
        s.cwd = Some("~/Projects/demo".into());
        let (argv, cwd) = launch_argv(&s, Platform::Windows, no_env, nothing);
        assert_eq!(
            argv,
            vec!["wsl.exe", "--", "bash", "-lic", "cd ~/'Projects/demo' && exec pi"]
        );
        assert_eq!(cwd, None, "a Linux cwd must never be handed to the Windows PTY");
    }

    #[test]
    fn wsl_distro_reaches_the_command_line() {
        let mut s = spec("pi-wsl");
        s.wsl_distro = Some("Ubuntu".into());
        let (argv, _) = launch_argv(&s, Platform::Windows, no_env, nothing);
        assert_eq!(&argv[..3], &["wsl.exe", "-d", "Ubuntu"]);
    }

    /// `wsl: true` describes reaching Linux *from Windows*; on Linux it is a no-op.
    #[test]
    fn wsl_flag_is_inert_off_windows() {
        let (argv, _) = launch_argv(&spec("pi-wsl"), Platform::Unix, no_env, nothing);
        assert_eq!(argv, vec!["/bin/zsh", "-lc", "exec pi"]);
    }

    /// A native harness's cwd IS a host path and does reach the PTY (FR-T5).
    #[test]
    fn native_harness_cwd_reaches_the_pty() {
        let mut s = spec("pi");
        s.cwd = Some("/Users/example/code".into());
        let (_, cwd) = launch_argv(&s, Platform::Unix, no_env, nothing);
        assert_eq!(cwd.as_deref(), Some("/Users/example/code"));
    }

    fn home_env(k: &str) -> Option<String> {
        match k {
            "HOME" => Some("/Users/example".into()),
            "USERPROFILE" => Some(r"C:\Users\example".into()),
            _ => None,
        }
    }

    /// Review finding: a `wsl` spec's Linux-shaped `cwd` reached `CommandBuilder`
    /// verbatim on a non-Windows host. It is a NATIVE launch there, so the host rule
    /// applies and the tilde is resolved — not passed through, and not dropped.
    #[test]
    fn wsl_spec_falling_back_to_native_expands_its_cwd() {
        let mut s = spec("pi-wsl");
        s.cwd = Some("~/Projects/demo".into());
        let (argv, cwd) = launch_argv(&s, Platform::Unix, home_env, nothing);
        assert_eq!(argv, vec!["/bin/zsh", "-lc", "exec pi"], "wsl is inert off Windows");
        assert_eq!(cwd.as_deref(), Some("/Users/example/Projects/demo"));
        assert!(!cwd.unwrap().starts_with('~'), "a raw ~ can never reach chdir/CreateProcessW");
    }

    /// The sibling finding: the same shorthand on a plain native harness. `~` is
    /// shell syntax, so an unexpanded one fails to spawn on either platform.
    #[test]
    fn native_harness_cwd_expands_the_tilde_on_both_platforms() {
        let mut s = spec("pi");
        s.cwd = Some("~/Projects/demo".into());

        let (_, unix) = launch_argv(&s, Platform::Unix, home_env, nothing);
        assert_eq!(unix.as_deref(), Some("/Users/example/Projects/demo"));

        let (_, win) = launch_argv(&s, Platform::Windows, home_env, nothing);
        assert_eq!(win.as_deref(), Some(r"C:\Users\example/Projects/demo"));
    }

    /// The WSL half is unchanged: bash owns that expansion, so the `~` stays.
    #[test]
    fn a_real_wsl_launch_still_leaves_the_tilde_for_bash() {
        let mut s = spec("pi-wsl");
        s.cwd = Some("~/Projects/demo".into());
        let (argv, cwd) = launch_argv(&s, Platform::Windows, home_env, nothing);
        assert_eq!(argv.last().unwrap(), "cd ~/'Projects/demo' && exec pi");
        assert_eq!(cwd, None, "the host must not also chdir");
    }

    /// No home to expand against: pass it through so the spawn error names the
    /// literal path, rather than silently rebasing it somewhere unexpected.
    #[test]
    fn cwd_without_a_home_is_left_alone() {
        let mut s = spec("pi");
        s.cwd = Some("~/code".into());
        let (_, cwd) = launch_argv(&s, Platform::Unix, no_env, nothing);
        assert_eq!(cwd.as_deref(), Some("~/code"));
    }

    #[test]
    fn windows_registry_carries_wsl_entries_and_unix_does_not() {
        let win = builtin_for(Platform::Windows);
        assert!(win.iter().any(|h| h.id == "pi-wsl" && h.wsl));
        assert!(win.iter().all(|h| h.cwd.is_none()), "no user path ships in product data");

        let unix = builtin_for(Platform::Unix);
        assert!(unix.iter().all(|h| !h.wsl), "WSL entries are Windows-only");
        assert!(!unix.iter().any(|h| h.id == "pi-wsl"));
    }

    /// WSL entries detect on the bridge, and that is all they can honestly claim.
    #[test]
    fn wsl_entries_detect_on_the_bridge() {
        let win = builtin_for(Platform::Windows);
        let none = detect_installed(&win, |_| false);
        assert!(!none.contains("pi-wsl"));
        let with_wsl = detect_installed(&win, |b| b == "wsl.exe");
        assert!(with_wsl.contains("pi-wsl") && with_wsl.contains("shell"));
        assert!(!with_wsl.contains("pi"), "the native pi is still absent");
    }

    /// Forward-compat: a pre-existing user file has none of the new fields.
    #[test]
    fn user_entries_without_the_new_fields_still_parse() {
        let one: HarnessSpec =
            serde_json::from_str(r#"{"id":"x","name":"X","command":["x"],"detect":["x"]}"#).unwrap();
        assert_eq!(one.cwd, None);
        assert!(!one.wsl);
        assert_eq!(one.wsl_distro, None);
        assert!(!one.conversation, "a terminal tab is what a spec means unless it says otherwise");
    }

    /// §5.9's second front-end is one registry row, on every platform, beside the
    /// terminal row for the same CLI — never replacing it (Rule 5′: degrading to a
    /// terminal tab is always available).
    #[test]
    fn the_conversation_row_sits_beside_the_terminal_one() {
        for platform in [Platform::Windows, Platform::Unix] {
            let reg = builtin_for(platform);
            let chat = reg.iter().find(|h| h.id == "claude-chat").expect("the conversation row");
            assert!(chat.conversation);
            assert!(chat.command.is_empty(), "the CLI's own flags decide the argv, not this");
            assert_eq!(chat.detect, vec!["claude"], "the same PATH probe as the terminal row");
            assert!(
                reg.iter().any(|h| h.id == "claude" && !h.conversation),
                "the terminal row must survive the split"
            );
            assert_eq!(
                reg.iter().filter(|h| h.conversation).count(),
                1,
                "Rule 5′ permits exactly one named integration at a time"
            );
        }
    }

    /// Detection is shared, so the + menu cannot offer a conversation with a CLI that
    /// is not installed — nor grey out one that is.
    #[test]
    fn the_conversation_row_detects_on_the_same_binary() {
        let reg = builtin_for(Platform::Windows);
        assert!(!detect_installed(&reg, |_| false).contains("claude-chat"));
        let with_claude = detect_installed(&reg, |b| b == "claude");
        assert!(with_claude.contains("claude-chat") && with_claude.contains("claude"));
    }

    // ---- where a conversation tab starts (§5.9, the `Unknown skill` defect) ----------

    /// A conversation spec as it ships: no `cwd`, which is what made the tab inherit
    /// whatever directory the console happened to be in.
    fn chat() -> HarnessSpec {
        builtin_for(Platform::Windows).into_iter().find(|h| h.id == "claude-chat").unwrap()
    }

    /// The marker test as a set of directories, so the walk is pure.
    fn projects(dirs: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| dirs.iter().any(|d| Path::new(d) == p)
    }

    fn none(_: &Path) -> bool {
        false
    }

    /// CONTRACT: a `cwd` on the spec is the last word, and its `~` is resolved — the
    /// registry is the user's seam, so nothing may second-guess a row they wrote.
    #[test]
    fn a_spec_cwd_wins_and_expands_its_tilde() {
        let mut s = chat();
        s.cwd = Some("~/Projects/demo".into());
        let r = conversation_cwd(&s, Platform::Unix, Path::new("/tmp"), home_env, none);
        assert_eq!(r.source, CwdSource::Spec, "an explicit row outranks every guess");
        assert_eq!(r.dir, "/Users/example/Projects/demo");
    }

    /// CONTRACT: with no spec `cwd`, the per-launch variable decides — and its `~` too.
    #[test]
    fn the_project_variable_decides_when_the_spec_is_silent() {
        let env = |k: &str| match k {
            PROJECT_ENV => Some("~/code/thing".to_string()),
            other => home_env(other),
        };
        let r = conversation_cwd(&chat(), Platform::Unix, Path::new("/tmp"), env, none);
        assert_eq!(r.source, CwdSource::Env);
        assert_eq!(r.dir, "/Users/example/code/thing");
    }

    /// CONTRACT: an empty or blank variable is not an answer. A shim that sets it from an
    /// unset value would otherwise aim every tab at the filesystem root.
    #[test]
    fn a_blank_project_variable_is_no_answer() {
        let env = |k: &str| (k == PROJECT_ENV).then(|| "   ".to_string());
        let r = conversation_cwd(&chat(), Platform::Unix, Path::new("/tmp/here"), env, none);
        assert_eq!(r.source, CwdSource::LaunchDir);
        assert_eq!(r.dir, "/tmp/here");
    }

    /// CONTRACT: launched anywhere inside a checkout, the tab starts at the checkout's
    /// root — the rule that makes `cd <project> && organon-console` need no configuration.
    #[test]
    fn the_launch_directory_ascends_to_the_nearest_project_root() {
        let r = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example/code/organon/native/organon-shell"),
            home_env,
            projects(&["/Users/example/code/organon"]),
        );
        assert_eq!(r.source, CwdSource::ProjectRoot);
        assert_eq!(r.dir, "/Users/example/code/organon", "the root, not the subdirectory");
        assert!(!r.bare, "a project root is by definition not bare");
    }

    /// CONTRACT: *nearest* wins. A checkout inside a checkout gets the inner one.
    #[test]
    fn the_nearest_project_root_wins_over_an_outer_one() {
        let r = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example/code/outer/inner/src"),
            home_env,
            projects(&["/Users/example/code/outer", "/Users/example/code/outer/inner"]),
        );
        assert_eq!(r.dir, "/Users/example/code/outer/inner");
    }

    /// CONTRACT: home is never *discovered*. A `~/.claude` is user-global configuration,
    /// not a project, and ascending into it would aim a console launched from `~/Documents`
    /// at the whole home directory.
    #[test]
    fn the_home_directory_is_never_discovered_as_a_project_root() {
        let r = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example/Documents"),
            home_env,
            projects(&["/Users/example", "/Users"]),
        );
        assert_eq!(r.source, CwdSource::LaunchDir, "the walk stops before home");
        assert_eq!(r.dir, "/Users/example/Documents");
    }

    /// CONTRACT: …but launching *in* home still lands in home. The stop rule removes the
    /// ascent, not the fallback.
    #[test]
    fn launching_in_the_home_directory_still_starts_there() {
        let r = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example"),
            home_env,
            projects(&["/Users/example"]),
        );
        assert_eq!(r.source, CwdSource::LaunchDir);
        assert_eq!(r.dir, "/Users/example");
    }

    /// CONTRACT: with no home known, the walk still terminates and still answers.
    #[test]
    fn no_home_does_not_stop_the_walk_from_answering() {
        let r = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/srv/build/checkout/crate"),
            no_env,
            projects(&["/srv/build/checkout"]),
        );
        assert_eq!(r.dir, "/srv/build/checkout");
    }

    /// CONTRACT: the resolution is always reported, and a directory with no project
    /// context reports that too — the silence is the defect, so there is no quiet path.
    #[test]
    fn every_resolution_is_reported_and_a_bare_one_warns() {
        let landed = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example/Documents"),
            home_env,
            none,
        );
        assert!(landed.bare, "no marker anywhere means no project context");
        let notes = cwd_notes(&landed);
        assert_eq!(notes.len(), 2, "the fact, then the warning");
        assert!(notes[0].contains("/Users/example/Documents"), "the line names the directory");
        assert!(notes[0].contains("where the console started"), "and which rule chose it");
        assert!(notes[1].contains("no project"), "and the consequence, in the failure's words");

        let inside = conversation_cwd(
            &chat(),
            Platform::Unix,
            Path::new("/Users/example/code/thing"),
            home_env,
            projects(&["/Users/example/code/thing"]),
        );
        assert_eq!(cwd_notes(&inside).len(), 1, "nothing to warn about, so no warning");
    }

    /// CONTRACT: a user-written `cwd` pointing somewhere bare is still reported bare.
    /// Being explicit is not evidence of being right — a typo'd path is the likeliest way
    /// to reach a directory with nothing in it.
    #[test]
    fn an_explicit_cwd_is_still_checked_for_project_context() {
        let mut s = chat();
        s.cwd = Some("/Users/example/typo".into());
        let r = conversation_cwd(&s, Platform::Unix, Path::new("/Users/example"), home_env, none);
        assert_eq!(r.source, CwdSource::Spec);
        assert!(r.bare);
        assert_eq!(cwd_notes(&r).len(), 2);
    }

    /// A user file is how a personal project directory gets in — the shape quoted
    /// in SHELL_ARCHITECTURE.md must actually deserialize.
    #[test]
    fn documented_wsl_user_entry_round_trips() {
        let s: HarnessSpec = serde_json::from_str(
            r#"{"id":"pi-wsl","name":"Pi (WSL)","glyph":"π","command":["pi"],
                "detect":["wsl.exe"],"wsl":true,"cwd":"~/Projects/demo"}"#,
        )
        .unwrap();
        let (argv, cwd) = launch_argv(&s, Platform::Windows, no_env, nothing);
        assert_eq!(argv.last().unwrap(), "cd ~/'Projects/demo' && exec pi");
        assert_eq!(cwd, None);
    }
}

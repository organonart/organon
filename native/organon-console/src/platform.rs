//! Platform-dependent process launching (Console #10 T1 follow-up, 2026-08-08).
//!
//! Everything about *how a tab starts a process* differs between Windows and the
//! POSIX platforms: what the user's shell is, how you ask it to run a command, what
//! counts as an executable on `PATH`, and — on Windows specifically — whether the
//! harness even lives in the same filesystem namespace as the app (WSL).
//!
//! ⚠️ **Why this is a module of pure functions taking [`Platform`] as a VALUE,
//! rather than a scattering of `#[cfg(windows)]`.** the Console shipped a terminal that
//! could not open a shell on Windows, and the Windows CI leg was **green the whole
//! time**. The reason is exactly the shape of the old code: the POSIX defaults were
//! `#[cfg]`-free literals (`/bin/zsh`, `-lc`) on a path only reached when the caller
//! passes no explicit command — and every test passes an explicit command, so the
//! default-shell branch had no coverage on any platform, and `#[cfg(windows)]` code
//! could not have been tested from a Mac even if it had existed.
//!
//! Taking the platform as a parameter fixes both halves: the Windows decisions are
//! exercised by `cargo test` **on a Mac**, and the branch that actually failed is
//! the one under test. Production calls [`Platform::current`]; tests call both.
//! Add a platform-dependent decision *here*, with a test for each variant — never
//! as a `#[cfg]` at the point of use.

/// The launching platform. Not `target_os` at the point of use — a value, so both
/// arms are reachable from a test on either host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// macOS, Linux, and anything else with a POSIX shell.
    Unix,
    Windows,
}

impl Platform {
    /// What this build is actually running on.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        }
    }
}

/// The argv for "the user's own shell, interactively".
///
/// **Unix** honours `$SHELL` and falls back to `/bin/zsh`, launched as a **login**
/// shell (`-l`) so the real rc environment loads — the thing that makes `nvm`,
/// `asdf`, and a hand-rolled `PATH` behave the way they do in the user's own
/// terminal.
///
/// **Windows** has no `$SHELL`, and reading one would be wrong even if something set
/// it (a Git-Bash `$SHELL` of `/usr/bin/bash` is not a path `CreateProcessW` can
/// use). The preference order is PowerShell 7 → Windows PowerShell → `%ComSpec%` →
/// `cmd.exe`, decided by `have`, and there is **no `-l`**: it is not a login-shell
/// concept, and `powershell -l` is not the flag anyone means.
pub fn default_shell(
    platform: Platform,
    env: impl Fn(&str) -> Option<String>,
    have: impl Fn(&str) -> bool,
) -> Vec<String> {
    match platform {
        Platform::Unix => {
            let shell = env("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());
            vec![shell, "-l".into()]
        }
        Platform::Windows => {
            for candidate in ["pwsh.exe", "powershell.exe"] {
                if have(candidate) {
                    // -NoLogo: the banner is noise in a terminal that is already ours.
                    return vec![candidate.into(), "-NoLogo".into()];
                }
            }
            let comspec = env("ComSpec").filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".into());
            vec![comspec]
        }
    }
}

/// How to launch a **harness** command so the PATH that resolves it is the user's.
///
/// The two platforms need genuinely different treatment, and the reason is not
/// cosmetic:
///
/// * **Unix** wraps in the login shell (`<shell> -lc "exec …"`) because a harness
///   installed by `nvm`/`asdf`/Homebrew is frequently not on the PATH a GUI app
///   inherits. `exec` keeps the process tree flat so signals and exit reach the
///   harness, not a wrapper.
/// * **Windows** wraps in `cmd.exe /C` for a different reason: npm-installed CLIs
///   (`pi`, `claude`, `codex`) land as **`.cmd` shims**, which `CreateProcessW`
///   *cannot execute directly* — only a shell resolves `PATHEXT`. Spawning the bare
///   argv would fail with the same "cannot find the path specified" that sent us
///   here. There is no `exec` equivalent; `cmd` stays in the tree as a thin parent.
pub fn wrap_harness(
    platform: Platform,
    argv: &[String],
    env: impl Fn(&str) -> Option<String>,
    have: impl Fn(&str) -> bool,
) -> Vec<String> {
    if argv.is_empty() {
        return default_shell(platform, env, have);
    }
    match platform {
        Platform::Unix => {
            let shell = env("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());
            vec![shell, "-lc".into(), format!("exec {}", argv.join(" "))]
        }
        Platform::Windows => {
            let comspec = env("ComSpec").filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".into());
            vec![comspec, "/C".into(), argv.join(" ")]
        }
    }
}

/// Run one shell command string — the `sh -c` idiom — on either platform.
///
/// Distinct from [`wrap_harness`] because the payload is a *script*, not an argv:
/// wrapping it with `exec` would apply the exec to only the first command of a
/// compound line and quietly change what runs.
pub fn shell_dash_c(
    platform: Platform,
    script: &str,
    env: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    match platform {
        Platform::Unix => vec!["/bin/sh".into(), "-c".into(), script.into()],
        Platform::Windows => {
            let comspec = env("ComSpec").filter(|s| !s.is_empty()).unwrap_or_else(|| "cmd.exe".into());
            vec![comspec, "/C".into(), script.into()]
        }
    }
}

/// Launch a harness **inside WSL** from the Windows host (`wsl.exe`).
///
/// This is not a convenience wrapper — it is the only way a Windows Console can host a
/// Linux-side agent, which for the Pi workflow is the *normal* case rather than an
/// exotic one. Three details are load-bearing:
///
/// * **`bash -lic`, not `bash -c`.** Login *and* interactive: `-l` runs `.profile`,
///   `-i` runs `.bashrc`, and on a typical WSL box `nvm` (hence `pi`) is initialised
///   from `.bashrc`. `-c` alone finds nothing and reports "command not found".
/// * **The `cd` happens inside WSL**, never as the PTY's working directory. A
///   `HarnessSpec::cwd` for a WSL harness is a *Linux* path (`~/Projects/demo`);
///   handing that to `CommandBuilder::cwd` on the Windows side is meaningless.
/// * **`exec`** so the harness replaces the shell and owns the PTY.
///
/// `distro` selects `wsl -d <name>`; `None` uses the user's default distribution.
pub fn wrap_wsl(argv: &[String], cwd: Option<&str>, distro: Option<&str>) -> Vec<String> {
    let mut out = vec!["wsl.exe".to_string()];
    if let Some(d) = distro.filter(|d| !d.is_empty()) {
        out.push("-d".into());
        out.push(d.into());
    }
    let script = match cwd.filter(|c| !c.is_empty()) {
        Some(dir) => format!("{} && exec {}", bash_cd(dir), argv.join(" ")),
        None => format!("exec {}", argv.join(" ")),
    };
    out.extend(["--".into(), "bash".into(), "-lic".into(), script]);
    out
}

/// A `cd` command for bash that survives spaces **and** still expands a leading `~`.
///
/// The subtlety that makes this its own function: `cd '~/x'` does **not** expand the
/// tilde — quoting defeats it — so the naive "quote the whole path" answer silently
/// fails for exactly the paths people actually write. The leading `~` (or `~/`) is
/// therefore left bare and only the remainder is quoted.
pub fn bash_cd(dir: &str) -> String {
    if dir == "~" {
        return "cd ~".into();
    }
    match dir.strip_prefix("~/") {
        Some(rest) => format!("cd ~/{}", single_quote(rest)),
        None => format!("cd {}", single_quote(dir)),
    }
}

/// POSIX single-quoting: wrap in `'…'`, and end/reopen the quote around any `'`.
fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The user's home directory, read from the environment.
///
/// From the env rather than `dirs::home_dir()` so it stays injectable — the same
/// reason [`Platform`] is a value. `HOME` on Unix, `USERPROFILE` on Windows.
pub fn home_dir(platform: Platform, env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let key = match platform {
        Platform::Unix => "HOME",
        Platform::Windows => "USERPROFILE",
    };
    env(key).filter(|s| !s.is_empty())
}

/// Expand a leading `~` / `~/` against `home`, for a path being handed to the OS.
///
/// ⚠️ **`~` is shell syntax, not filesystem syntax.** Neither `chdir` nor
/// `CreateProcessW` expands it, so a `harnesses.json` entry of
/// `"cwd": "~/Projects/demo"` — the exact shorthand this crate's own docs and
/// tests use — fails to spawn on both platforms unless expanded here first.
///
/// This is the **native** counterpart to [`bash_cd`], and the pair implements one
/// rule: *a `cwd` is a path in whatever namespace the process actually starts in.*
/// A WSL launch starts inside WSL, so its `cd` is written for bash and `~` is left
/// for bash to expand. A native launch starts on this host, so `~` must be resolved
/// before the OS ever sees it. Without both halves the shorthand works in one place
/// and silently fails in the other.
///
/// With no `home` available the path is returned unchanged: a spawn failure naming
/// a literal `~/…` is a better outcome than a path silently rebased somewhere else.
pub fn expand_tilde(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|h| !h.is_empty()) else { return path.to_string() };
    if path == "~" {
        return home.to_string();
    }
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        // Trim a trailing separator on $HOME so we never emit a doubled one.
        Some(rest) => format!("{}/{}", home.trim_end_matches(['/', '\\']), rest),
        // `~other` is another user's home, which only a shell can resolve. Left alone.
        None => path.to_string(),
    }
}

/// The filenames that could be `bin` as an executable on this platform.
///
/// On Unix that is just `bin`. On Windows a bare name is almost never the file: the
/// shell resolves it through `PATHEXT`, so `pi` on disk is `pi.exe`, `pi.cmd` or
/// `pi.bat`. Probing only the bare name is why **every harness showed as
/// not-installed** on Windows — the registry's `detect` entries are bare names.
pub fn executable_names(platform: Platform, bin: &str, env: impl Fn(&str) -> Option<String>) -> Vec<String> {
    match platform {
        Platform::Unix => vec![bin.to_string()],
        Platform::Windows => {
            // An explicit extension is already a filename; don't append to it.
            if bin.contains('.') {
                return vec![bin.to_string()];
            }
            let pathext = env("PATHEXT")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            let mut names = vec![bin.to_string()];
            names.extend(
                pathext
                    .split(';')
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .map(|ext| format!("{bin}{}", ext.to_ascii_lowercase())),
            );
            names
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }
    fn nothing(_: &str) -> bool {
        false
    }

    #[test]
    fn unix_default_shell_honours_shell_then_falls_back() {
        assert_eq!(
            default_shell(Platform::Unix, |k| (k == "SHELL").then(|| "/bin/fish".into()), nothing),
            vec!["/bin/fish", "-l"]
        );
        assert_eq!(default_shell(Platform::Unix, no_env, nothing), vec!["/bin/zsh", "-l"]);
        // An empty $SHELL is not a shell.
        assert_eq!(
            default_shell(Platform::Unix, |_| Some(String::new()), nothing),
            vec!["/bin/zsh", "-l"]
        );
    }

    /// The regression that sent us here: on Windows the default tab tried to run
    /// `/bin/zsh -l` and died with os error 3. Nothing here may ever be a POSIX path.
    #[test]
    fn windows_default_shell_is_never_a_posix_path() {
        let pwsh = default_shell(Platform::Windows, no_env, |c| c == "pwsh.exe");
        assert_eq!(pwsh, vec!["pwsh.exe", "-NoLogo"]);

        let ps5 = default_shell(Platform::Windows, no_env, |c| c == "powershell.exe");
        assert_eq!(ps5, vec!["powershell.exe", "-NoLogo"]);

        // Neither PowerShell present: ComSpec, then cmd.exe.
        assert_eq!(
            default_shell(Platform::Windows, |k| (k == "ComSpec").then(|| r"C:\W\cmd.exe".into()), nothing),
            vec![r"C:\W\cmd.exe"]
        );
        assert_eq!(default_shell(Platform::Windows, no_env, nothing), vec!["cmd.exe"]);

        // A stray $SHELL (Git Bash sets one) must not be believed on Windows.
        let stray = default_shell(Platform::Windows, |k| (k == "SHELL").then(|| "/usr/bin/bash".into()), nothing);
        assert!(!stray[0].starts_with('/'), "a POSIX $SHELL leaked into Windows: {stray:?}");
    }

    #[test]
    fn pwsh_is_preferred_over_powershell() {
        let both = default_shell(Platform::Windows, no_env, |_| true);
        assert_eq!(both[0], "pwsh.exe", "PowerShell 7 wins when both are present");
    }

    #[test]
    fn harness_wrapping_per_platform() {
        let argv = vec!["pi".to_string()];
        assert_eq!(
            wrap_harness(Platform::Unix, &argv, no_env, nothing),
            vec!["/bin/zsh", "-lc", "exec pi"]
        );
        assert_eq!(
            wrap_harness(Platform::Windows, &argv, no_env, nothing),
            vec!["cmd.exe", "/C", "pi"]
        );
    }

    /// `.cmd` shims are the reason for the `cmd.exe /C` wrap; assert the wrap exists
    /// rather than trusting a comment about it.
    #[test]
    fn windows_harness_goes_through_a_shell_so_pathext_resolves() {
        let w = wrap_harness(Platform::Windows, &["claude".to_string()], no_env, nothing);
        assert_eq!(w[0].to_ascii_lowercase(), "cmd.exe");
        assert_eq!(w[1], "/C");
    }

    #[test]
    fn empty_argv_falls_back_to_the_plain_shell() {
        assert_eq!(
            wrap_harness(Platform::Unix, &[], no_env, nothing),
            default_shell(Platform::Unix, no_env, nothing)
        );
        assert_eq!(
            wrap_harness(Platform::Windows, &[], no_env, nothing),
            default_shell(Platform::Windows, no_env, nothing)
        );
    }

    #[test]
    fn wsl_wrapping_is_login_and_interactive() {
        let w = wrap_wsl(&["pi".to_string()], None, None);
        assert_eq!(w, vec!["wsl.exe", "--", "bash", "-lic", "exec pi"]);
        // -i is not decoration: nvm (hence pi) is initialised from .bashrc.
        assert!(w.contains(&"-lic".to_string()), "must be login AND interactive");
    }

    #[test]
    fn wsl_cd_runs_inside_wsl_and_execs() {
        let w = wrap_wsl(&["pi".to_string()], Some("~/Projects/demo"), None);
        assert_eq!(w.last().unwrap(), "cd ~/'Projects/demo' && exec pi");
    }

    #[test]
    fn wsl_distro_is_selectable() {
        let w = wrap_wsl(&["pi".to_string()], None, Some("Ubuntu-22.04"));
        assert_eq!(&w[..3], &["wsl.exe", "-d", "Ubuntu-22.04"]);
        let none = wrap_wsl(&["pi".to_string()], None, Some(""));
        assert_eq!(&none[..2], &["wsl.exe", "--"], "an empty distro means the default");
    }

    /// The tilde trap: quoting the whole path defeats `~` expansion, so a naive
    /// implementation breaks on precisely the paths users write.
    #[test]
    fn bash_cd_expands_tilde_and_survives_spaces() {
        assert_eq!(bash_cd("~"), "cd ~");
        assert_eq!(bash_cd("~/Projects/demo"), "cd ~/'Projects/demo'");
        assert_eq!(bash_cd("~/my projects/pi"), "cd ~/'my projects/pi'");
        assert_eq!(bash_cd("/opt/agents"), "cd '/opt/agents'");
        assert_eq!(bash_cd("/opt/my agents"), "cd '/opt/my agents'");
        assert!(!bash_cd("~/x").starts_with("cd '~"), "a quoted tilde never expands");
    }

    #[test]
    fn bash_cd_escapes_quotes() {
        assert_eq!(bash_cd("/tmp/it's"), r"cd '/tmp/it'\''s'");
    }

    /// Why every harness was greyed out on Windows: `detect` holds bare names, and
    /// the real file is `pi.cmd`.
    #[test]
    fn windows_executable_names_cover_pathext() {
        let names = executable_names(Platform::Windows, "pi", no_env);
        assert!(names.contains(&"pi.exe".to_string()));
        assert!(names.contains(&"pi.cmd".to_string()));
        assert!(names.contains(&"pi.bat".to_string()));
        assert!(names.contains(&"pi".to_string()), "the bare name stays a candidate");

        assert_eq!(executable_names(Platform::Unix, "pi", no_env), vec!["pi"]);
    }

    #[test]
    fn pathext_is_honoured_and_an_explicit_extension_is_left_alone() {
        let names = executable_names(Platform::Windows, "pi", |k| {
            (k == "PATHEXT").then(|| ".EXE;.PS1".into())
        });
        assert_eq!(names, vec!["pi", "pi.exe", "pi.ps1"]);

        assert_eq!(
            executable_names(Platform::Windows, "wsl.exe", no_env),
            vec!["wsl.exe"],
            "an explicit extension must not gain a second one"
        );
    }

    #[test]
    fn home_dir_reads_the_right_variable_per_platform() {
        let env = |k: &str| match k {
            "HOME" => Some("/Users/example".to_string()),
            "USERPROFILE" => Some(r"C:\Users\example".to_string()),
            _ => None,
        };
        assert_eq!(home_dir(Platform::Unix, env).as_deref(), Some("/Users/example"));
        assert_eq!(home_dir(Platform::Windows, env).as_deref(), Some(r"C:\Users\example"));
        assert_eq!(home_dir(Platform::Unix, no_env), None);
        assert_eq!(home_dir(Platform::Unix, |_| Some(String::new())), None, "empty is not a home");
    }

    /// `~` is shell syntax; `chdir`/`CreateProcessW` never expand it. The WSL side
    /// already handled the same shorthand, so leaving the native side raw made the
    /// documented `"cwd": "~/…"` work in one place and fail in the other.
    #[test]
    fn expand_tilde_resolves_the_documented_shorthand() {
        assert_eq!(expand_tilde("~/Projects/demo", Some("/Users/example")), "/Users/example/Projects/demo");
        assert_eq!(expand_tilde("~", Some("/Users/example")), "/Users/example");
        assert_eq!(
            expand_tilde(r"~\Github", Some(r"C:\Users\example")),
            r"C:\Users\example/Github",
            "a Windows-style tilde path still resolves"
        );
    }

    #[test]
    fn expand_tilde_leaves_everything_else_alone() {
        assert_eq!(expand_tilde("/opt/agents", Some("/Users/example")), "/opt/agents");
        assert_eq!(expand_tilde(r"C:\code", Some(r"C:\Users\example")), r"C:\code");
        // `~other` is another user's home — only a shell resolves that.
        assert_eq!(expand_tilde("~root/x", Some("/Users/example")), "~root/x");
        // No home: unchanged, so the failure names the literal path.
        assert_eq!(expand_tilde("~/x", None), "~/x");
        assert_eq!(expand_tilde("~/x", Some("")), "~/x");
    }

    #[test]
    fn expand_tilde_never_doubles_the_separator() {
        assert_eq!(expand_tilde("~/x", Some("/Users/example/")), "/Users/example/x");
        assert_eq!(expand_tilde("~/x", Some(r"C:\Users\example\")), r"C:\Users\example/x");
    }

    #[test]
    fn current_platform_matches_the_build() {
        let expected = if cfg!(windows) { Platform::Windows } else { Platform::Unix };
        assert_eq!(Platform::current(), expected);
    }
}

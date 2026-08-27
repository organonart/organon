//! **The four verbs behind "approve it as a module"** — T3b, and the tier where the trust
//! boundary stops being a diagram.
//!
//! [`crate::module`] (T3a) is the data: two files, two authors, and an approval a manifest
//! cannot write for itself. Nothing there starts a process. **This is the half that does** —
//! `git`, `cargo`, a checkout on the disk — and every decision it makes is still a pure
//! function of a string, because the two programs it runs sit behind [`Workshop`].
//!
//! `doc/organon_module_viewport.md` §3 is the specification. Four verbs:
//!
//! | | |
//! |---|---|
//! | [`approve`] | fetch a repository at a commit, show what its manifest **requests**, and — only if a person said which — record an approval holding what they **granted** |
//! | [`build`] | build the approved commit, and record **the commit that was actually built** plus whether the tree was dirty |
//! | [`compare`] | 📌 §3.3, and the reason the whole approach is git-based: *"show me what changed and ask again"* |
//! | revoke | [`crate::module::ModuleRegistry::revoke`] — not here, because it runs no program. See *"Revocation is not in this file"* below |
//!
//! # 🚨 Approving grants **build-time** trust, and this is where that is said out loud
//!
//! §3.4 names a hole and forbids papering over it. *"The process is the boundary"* is true of
//! a hosted module at **run** time and false of it at **build** time: pointing Organon at a
//! repository means Organon runs `cargo build` on it, and `build.rs` plus every proc macro in
//! its dependency graph execute **with the privileges of whoever is at the keyboard**, before
//! a single pixel is composited.
//!
//! Three things follow, and all three are in the code rather than only here:
//!
//! 1. **[`BUILD_TRUST`] is in the sentence a person reads at the moment they approve**, not in
//!    a document they may have read once. A grant somebody does not understand is not a grant.
//! 2. **There is no check that implies coverage it cannot have.** §3.4 rejects that explicitly,
//!    so nothing in this file scans a `build.rs`, greps for a proc macro, or reports a
//!    repository as clean. It fetches, it says what the repository asks for, and it says what
//!    building will cost.
//! 3. ⚠️ **[`approve`] never builds.** Cloning and reading a file out of a git object database
//!    execute none of the module's code, so the dry run — and the recording step too — are
//!    inert with respect to the thing that is actually dangerous. `cargo` is reached from
//!    exactly one function in this file, and it is [`build`].
//!
//! # 🚨 A manifest requests; it never grants — and here it cannot even name a program
//!
//! [`crate::module`] makes the first half structural. This file adds the half that only exists
//! once something runs: [`Tool`] is a **two-variant enum**, so the set of programs the console
//! will start is fixed at compile time and no field of any manifest reaches
//! [`std::process::Command::new`]. A manifest that could name its own build command would be a
//! manifest that grants itself arbitrary execution under the word "approve".
//!
//! The same rule one layer down: the *arguments* a manifest influences are the producer name
//! (a directory component, gated by [`crate::module::check_producer_name`], which grew two path
//! rules for exactly this) and nothing else. The URL and the reference come from the person
//! typing, and the commit is resolved by `git` rather than trusted from anywhere.
//!
//! # Nothing here may touch the frame thread
//!
//! A clone is a network, a build is a compiler, and the console draws sixty times a second.
//! §1.13's exhibit established the pattern this follows rather than inventing a second one:
//! **one thread per job, results on an `mpsc`, and a frame that only ever collects.** Every
//! function here is therefore written to be run on a worker thread and to return a value —
//! none of them touch `modules.json`, and that is deliberate as well as convenient: the
//! registry is read and written on the frame thread only, so a job and the console can never
//! be two writers of one file.
//!
//! # Revocation is not in this file, and the absence is the design
//!
//! §3.5's rule is absolute: **a layout referencing a module you have stopped trusting must not
//! fail to open.** Revoking is [`crate::module::ModuleRegistry::revoke`] plus a save — no
//! network, no compiler, microseconds — so it runs **synchronously** on the frame thread. That
//! is not an optimisation. The one verb whose whole purpose is to withdraw trust must not be
//! able to be queued behind a build, fail because a worker thread died, or need anything to be
//! reachable; and what a revoked producer then produces is
//! [`crate::module::ModuleState::NotApproved`] — a sentence in a rectangle — never a load
//! failure. Nothing in this file is on that path, which is the point.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::module::{
    ApprovedModule, BuildRecord, ModuleFault, ModuleManifest, ModuleSource, Requested,
    MANIFEST_FILE,
};

/// The directory under the store root that holds every module's checkout.
///
/// A sibling of `modules.json` rather than a directory somewhere else, so that "what has been
/// approved" and "what has been fetched" are one place a person can look at, back up and
/// delete.
pub const MODULES_DIR: &str = "modules";

/// The word that means *"approve it, and grant it nothing"*.
///
/// A grant list is comma-separated and the wire is whitespace-delimited, so the **empty** list
/// has no spelling of its own — hence a word. ⚠️ The cost, stated rather than discovered: a
/// module requesting a grant literally called `none` cannot be granted it by this spelling.
/// That is a real limit and it is the cheap end of the trade; the alternative is a grant list
/// whose empty case is invisible, and an invisible empty case in a permission system is worse
/// than an unreachable name.
pub const NONE_GRANT: &str = "none";

/// The action words `console module` takes, in the order a person meets them.
///
/// The one table, read by clap's `PossibleValuesParser`, by the slash grammar's `Choice` ring
/// and by [`ModuleCmd::resolve`] — so a fifth verb is one line here and not four.
pub const MODULE_ACTIONS: &[&str] = &["approve", "build", "diff", "revoke", "restart"];

/// 🚨 **What approving actually costs, in the words a person reads while they do it.**
///
/// §3.4. Kept as a constant because it belongs in three sentences — the dry run's, the
/// recorded approval's, and the build's — and three copies of a security disclosure is three
/// chances for one of them to soften.
pub const BUILD_TRUST: &str = "building a module runs its build scripts and every procedural \
                               macro in its dependency graph with your privileges, before any \
                               of it is composited — approving a repository trusts the people \
                               who write it, not only the picture it draws";

/// How many changed paths a comparison names before it starts counting instead.
///
/// §3.3's example sentence is *"changed 14 files since the commit you last trusted. Here they
/// are."* — so the list is the answer and a ceiling only exists because a rename across a
/// vendored directory can be four thousand of them.
const MAX_LISTED: usize = 40;

/// How many trailing lines of a failed program's output a refusal carries.
///
/// A `cargo build` failure is a wall; its **last** lines are the ones that say what broke.
const TAIL_LINES: usize = 12;

// ---------------------------------------------------------------------------------------
// The two programs, and the seam that makes every decision here headless
// ---------------------------------------------------------------------------------------

/// The programs this console will start on a module's behalf. **Exactly two, forever, by
/// being an enum.**
///
/// See this module's header: the value of a closed set here is that no string out of a
/// manifest — or out of `modules.json`, which is a file a person can edit — can become the
/// first argument to [`std::process::Command::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Git,
    Cargo,
}

impl Tool {
    /// The program name, resolved through the caller's `PATH` by the OS.
    pub fn program(self) -> &'static str {
        match self {
            Tool::Git => "git",
            Tool::Cargo => "cargo",
        }
    }

    /// Where a person gets it, for the refusal that says it is missing.
    pub fn install_hint(self) -> &'static str {
        match self {
            Tool::Git => "git is how a module is fetched at all — install it and put it on PATH",
            Tool::Cargo => {
                "cargo builds the module — install the Rust toolchain from https://rustup.rs"
            }
        }
    }
}

/// What running one finished with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolOutput {
    /// `None` when the program was killed by a signal, which is neither success nor a status.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl ToolOutput {
    /// A successful run, for a test that only cares what the program said.
    pub fn ok(stdout: &str) -> Self {
        ToolOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    /// A failed run carrying the message the program printed.
    pub fn failed(stderr: &str) -> Self {
        ToolOutput { status: Some(1), stdout: String::new(), stderr: stderr.into() }
    }

    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }

    /// `stdout` with its surrounding whitespace gone — every `git` answer this file reads is
    /// one line, and every one of them arrives with a newline on it.
    pub fn line(&self) -> &str {
        self.stdout.trim()
    }

    /// The last [`TAIL_LINES`] non-empty lines of whatever the program said, for a refusal.
    ///
    /// `stderr` first because that is where both programs put their diagnosis, falling back to
    /// `stdout` because a `cargo` that failed early can be silent on the other stream — and a
    /// refusal that quotes nothing is the "something went wrong" this tier is not allowed to
    /// produce.
    pub fn tail(&self) -> String {
        let source = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        let lines: Vec<&str> = source.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
        let start = lines.len().saturating_sub(TAIL_LINES);
        lines[start..].join("\n")
    }
}

/// Where `git` and `cargo` come from, and where a directory comes from.
///
/// 🚨 **The one injection point, on [`crate::harness`]'s precedent** — its PATH probe sits
/// behind an injectable lookup *"so every decision here is testable without touching the
/// machine's real PATH"*. Same rule, higher stakes: a test that really cloned a repository
/// would fail on a machine with no network, and a test that really built one would take
/// minutes. Every function in this file takes `&dyn Workshop`, so the whole of its judgement
/// is exercised against scripted output.
///
/// `Send + Sync` because a job runs on its own thread (see the header).
pub trait Workshop: Send + Sync {
    /// Run one program to completion in `cwd` and collect what it said.
    ///
    /// `Err` is for *"the program did not run"*; a program that ran and failed is `Ok` with a
    /// non-zero status, because the two want different sentences and only the caller knows
    /// which failure it was looking at.
    fn run(&self, tool: Tool, args: &[&str], cwd: &Path) -> Result<ToolOutput, WorkFault>;

    /// Make `path` exist, with its parents. On this trait rather than called directly so that
    /// a job's filesystem reach is the same seam as its process reach — a test drives both
    /// with one fake and can assert on either.
    fn ensure_dir(&self, path: &Path) -> Result<(), WorkFault>;

    /// Is there a file at `path`?
    ///
    /// 🚨 **On the trait for `ensure_dir`'s reason exactly**, and it earned its place the hard
    /// way: [`build`] verifies the binary it was supposed to produce, and a first version asked
    /// the real filesystem — which broke three headless tests immediately, because they build
    /// against a scripted `/store` that has never existed on any disk. That is the seam telling
    /// the truth about itself. A decision this file makes must be a pure function of what the
    /// `Workshop` says, or *"every decision here is testable without touching the machine"* is a
    /// sentence rather than a property.
    ///
    /// ⚠️ **Deliberately a `bool` and not a `Result`.** The question is *"is the artifact
    /// there?"*, and every way of failing to answer it — no permission, a broken symlink, a path
    /// that is a directory — means the same thing to every caller: **not a binary I can launch.**
    /// An error case would make each call site invent a policy for a distinction none of them can
    /// act on.
    fn file_exists(&self, path: &Path) -> bool;

    /// **Start a module's own binary and do not wait for it** — T5's launch, and the third
    /// thing this seam exists for.
    ///
    /// 🚨 **A different shape from [`Workshop::run`], not a variant of it.** `run` starts a
    /// program that ends, and its whole value is what the program said. This starts a program
    /// that is *supposed* to outlive the call, and its value is a **handle**: nothing is
    /// collected, nothing is waited for, and the only two questions ever asked of the result
    /// are *has it exited* and *stop*. Folding the two together would mean a `run` that
    /// sometimes blocks for minutes and sometimes returns immediately, on the frame thread.
    ///
    /// 🚨 **`program` is a `Path`, and that is the one place a string outside [`Tool`]'s closed
    /// set reaches the OS.** It is not a loosening: the path is *derived* by
    /// [`module_binary`] from the store root and the producer name — the same rule and the
    /// same gate as [`artifact_dir`], with [`crate::module::check_producer_name`] between a
    /// manifest and a path component. **No field of any manifest is ever passed here.** A
    /// `binary = ` key in `organon-module.toml` would be a manifest granting itself arbitrary
    /// execution under the word "approve", which is the thing this file's header forbids, and
    /// it is forbidden by the *absence of the field* rather than by a check.
    ///
    /// `env` is the handoff — [`organon_module::CHANNEL_ENV`] and the IPC namespace. It is a
    /// list rather than the ambient environment because a launch has to be reproducible in a
    /// test that has not exported anything.
    fn spawn(
        &self,
        program: &Path,
        cwd: &Path,
        env: &[(&str, String)],
    ) -> Result<Box<dyn ModuleProcess>, WorkFault>;
}

/// **A launched module, from the console's side of the process boundary.**
///
/// 🚨 **Two questions and nothing else.** `organon-module`'s `presence.rs` writes down the
/// honest limit of the shared mapping — *"hung and exited-without-a-farewell are not
/// distinguishable from inside a shared mapping, and the thing that genuinely knows is the
/// process handle, which is the launcher's."* This is that handle, and [`ModuleProcess::exited`]
/// is the answer the channel cannot reach. Anything more (stdout, a signal, a working set)
/// would be the console learning what the module *is*.
pub trait ModuleProcess: Send {
    /// `Some` once the process has ended, carrying its exit code — or `Some(None)` when it was
    /// killed by a signal, which is neither a success nor a status. `None` while it is running.
    ///
    /// ⚠️ **Non-blocking, and it must stay that way**: this is called once per console frame.
    fn exited(&mut self) -> Option<Option<i32>>;

    /// **The operating system's id for this process**, or `None` once it has ended.
    ///
    /// 🚨 **Only for addressing the process in facilities the protocol deliberately does not
    /// carry** — today, its audio session (`crate::module_audio`). `organon_module::input`'s
    /// refusal table has no sound in either direction and must not grow one: a `Mute` verb would
    /// be the console *asking* a producer to be quiet, so a producer that ignored it could not be
    /// silenced. A pid lets the console turn the process down in the OS mixer instead, where the
    /// answer does not depend on the module's cooperation.
    ///
    /// ⚠️ **Not a handle, and not a way in.** Nothing here may use this to open, inject into, or
    /// otherwise reach inside the process; the one legitimate shape is *naming* it to an OS
    /// facility that already governs it from outside.
    fn pid(&self) -> Option<u32>;

    /// End it. Called when the console stops hosting this producer — the region was cleared,
    /// the approval was revoked, a restart was asked for, or the console is going away.
    ///
    /// 📌 **The kill is here and the polite request is on the channel.**
    /// `ModuleChannel::request_close` asks; this does not. A console that only ever asked would
    /// leak a process every time a module ignored it, and a console that only ever killed would
    /// deny a module the chance to shut down cleanly — so the host does both, in that order.
    fn kill(&mut self);
}

/// [`Workshop`] against the real machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessWorkshop;

impl Workshop for ProcessWorkshop {
    fn run(&self, tool: Tool, args: &[&str], cwd: &Path) -> Result<ToolOutput, WorkFault> {
        let output = std::process::Command::new(tool.program())
            .args(args)
            .current_dir(cwd)
            // 🚨 **A private repository behind a credential helper will otherwise HANG, and it
            // hangs in the worst available way**: on a worker thread, with no terminal
            // attached, waiting on a prompt nobody can see or answer, until the console is
            // killed. `GIT_TERMINAL_PROMPT=0` makes git fail instead of asking, and
            // `GIT_ASKPASS` covers the graphical helpers that ignore it — so an unauthorised
            // fetch becomes a refusal with git's own message in it, which is a sentence a
            // person can act on.
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "echo")
            // Colour codes in a refusal are noise on every surface this text reaches.
            .env("NO_COLOR", "1")
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    WorkFault::ToolMissing { tool }
                } else {
                    WorkFault::ToolFailed { tool, why: e.to_string() }
                }
            })?;
        Ok(ToolOutput {
            status: output.status.code(),
            // ⚠️ Lossy on purpose. A compiler diagnostic quoting a source file is not
            // guaranteed to be UTF-8, and a refusal that vanished because somebody's error
            // message had a stray byte in it would be the least diagnosable outcome available.
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn ensure_dir(&self, path: &Path) -> Result<(), WorkFault> {
        std::fs::create_dir_all(path)
            .map_err(|e| WorkFault::NoDirectory { path: path.display().to_string(), why: e.to_string() })
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn spawn(
        &self,
        program: &Path,
        cwd: &Path,
        env: &[(&str, String)],
    ) -> Result<Box<dyn ModuleProcess>, WorkFault> {
        let mut cmd = std::process::Command::new(program);
        cmd.current_dir(cwd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        // ⚠️ **Inherited, deliberately.** A module's own diagnostics are the only thing that
        // explains a producer which starts and draws nothing, and the console's stderr is where
        // every other subsystem here already reports. Capturing them would need a reader thread
        // per module to avoid filling a pipe and deadlocking the child — which is a thread and
        // a buffer bought to make a message harder to find.
        cmd.stdin(std::process::Stdio::null());
        let child = cmd.spawn().map_err(|e| WorkFault::LaunchFailed {
            path: program.display().to_string(),
            why: if e.kind() == std::io::ErrorKind::NotFound {
                // The most common cause by far, and the one whose OS message ("The system
                // cannot find the file specified") names neither the module nor the verb.
                format!("{e} — `{}` produces it", crate::module::BUILD_VERB)
            } else {
                e.to_string()
            },
        })?;
        Ok(Box::new(SpawnedProcess { child, ended: None }))
    }
}

/// [`ModuleProcess`] over a real [`std::process::Child`].
struct SpawnedProcess {
    child: std::process::Child,
    /// 🚨 **The answer is remembered, because `try_wait` only reports it once.** After it
    /// reaps, a second call returns `Ok(None)` — indistinguishable from "still running" — so a
    /// host that asked every frame would see the death for exactly one frame and then decide
    /// the process was fine again. That is a stale picture with extra steps.
    ended: Option<Option<i32>>,
}

impl ModuleProcess for SpawnedProcess {
    fn pid(&self) -> Option<u32> {
        // `None` once it has ended: the id is reusable the moment the process is reaped, so
        // handing it out afterwards is handing out somebody else's process.
        self.ended.is_none().then(|| self.child.id())
    }

    fn exited(&mut self) -> Option<Option<i32>> {
        if self.ended.is_none() {
            // An error from `try_wait` means the handle itself is unusable, which is not a
            // state to keep asking about: treat it as gone, with no status to report.
            match self.child.try_wait() {
                Ok(Some(status)) => self.ended = Some(Some(status.code().unwrap_or(-1))),
                Ok(None) => {}
                Err(_) => self.ended = Some(None),
            }
        }
        self.ended
    }

    fn kill(&mut self) {
        if self.ended.is_some() {
            return;
        }
        let _ = self.child.kill();
        // Reaped immediately so the child does not become a zombie on Unix — `kill` alone
        // signals, it does not collect.
        let _ = self.child.wait();
        self.ended = Some(None);
    }
}

// ---------------------------------------------------------------------------------------
// Refusals — every failure here is a person's next action
// ---------------------------------------------------------------------------------------

/// Why a verb could not finish.
///
/// 🚨 **Each of these is a different next action, so each gets its own sentence.** No network,
/// no toolchain, a commit that does not exist, a manifest that will not parse and a build that
/// failed are five different mornings; a shared *"something went wrong"* is what turns any of
/// them into an afternoon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkFault {
    /// The program is not on `PATH`.
    ToolMissing { tool: Tool },
    /// The program is there and would not start.
    ToolFailed { tool: Tool, why: String },
    /// A directory under the store root could not be made.
    NoDirectory { path: String, why: String },
    /// This platform has no data directory, so there is nowhere to put a checkout.
    NoStore,
    /// The producer name will not do — [`crate::module::check_producer_name`]'s answer, or one
    /// of the other faults T3a raises.
    Module(ModuleFault),
    /// `approve` with no repository, for a producer nothing is approved under.
    NoSource { producer: String },
    /// The clone did not happen.
    CloneFailed { url: String, why: String },
    /// The fetch did not happen.
    FetchFailed { url: String, reference: String, why: String },
    /// The reference resolved to nothing — a branch that is not there, a tag that moved, a
    /// hash the remote will not serve.
    NoSuchCommit { url: String, reference: String },
    /// Git answered with something that is not a commit hash. A belt: it means git's output
    /// changed shape, and taking it on trust would put a non-hash into `modules.json`.
    NotAHash { reference: String, answer: String },
    /// The commit has no `organon-module.toml`, so the repository is not offering itself as a
    /// module at all.
    NoManifest { producer: String, commit: String },
    /// 🚨 **The repository calls itself something else.** The person named a producer and the
    /// manifest at that commit disagrees, so approving would file a repository under a name
    /// its author did not choose — and a viewport asking for that name would run it.
    IdentityMismatch { asked: String, found: String },
    /// Nothing is approved under this name. `known` is what is, because a refusal that does
    /// not list the alternatives is a dead end.
    NotApproved { producer: String, known: Vec<String> },
    /// `cargo build` ran and did not succeed.
    BuildFailed { producer: String, tail: String },
    /// `git diff` ran and did not succeed.
    DiffFailed { producer: String, tail: String },
    /// 🚨 **T5: the module's own binary would not start.** Its own variant rather than
    /// [`WorkFault::ToolFailed`] because that one names a [`Tool`], and this program is not one
    /// — it is `<checkout>/target/release/<producer>`, which is the whole point.
    ///
    /// The overwhelmingly common cause is a `modules.json` claiming a build that is no longer
    /// on the disk: a `target` directory cleaned by hand, or a store restored from a backup
    /// that skipped it. `is_built()` reads the **record**, so it says yes and the file is gone.
    LaunchFailed { path: String, why: String },
    /// 🚨 **`cargo build --release` succeeded and produced no binary for this producer.**
    ///
    /// Its own variant rather than [`WorkFault::BuildFailed`], because that one quotes a
    /// compiler diagnostic and there is none: cargo skipped the target and exited 0. The two
    /// want different next actions — one is "fix the code", this one is "the repository does not
    /// produce the binary this contract requires", which is a manifest question.
    NoBinary { producer: String, expected: String },
    /// 🚨 **A binary is there and THIS build did not produce it.**
    ///
    /// The one the existence check above cannot see, and the more dangerous of the two: a binary
    /// left in `target/` by a previous commit satisfies `file_exists`, so the console records a
    /// built commit and then **launches bytes that are not that commit's** — which defeats §3.4's
    /// entire reason for recording a commit at all. Everything looks healthy.
    ///
    /// ⚠️ Its own variant rather than folding into [`WorkFault::NoBinary`], because the two want
    /// different next actions from a person. *No binary* says the **repository** does not meet
    /// the published contract. *This one* says the **checkout** is not what you think it is.
    StaleBinary { producer: String, expected: String },
    /// The frame channel could not be created — no `$TMPDIR`, a full disk, or a mapping the OS
    /// refused. Nothing can be hosted without it, so it is a refusal rather than a rectangle.
    ChannelFailed { producer: String, why: String },
}

impl From<ModuleFault> for WorkFault {
    fn from(f: ModuleFault) -> Self {
        WorkFault::Module(f)
    }
}

impl WorkFault {
    /// The one sentence this refusal is said in. [`ModuleFault::sentence`]'s rule and its
    /// reason in full: generated from the values that caused it, never written twice.
    pub fn sentence(&self) -> String {
        match self {
            WorkFault::ToolMissing { tool } => format!(
                "`{}` is not on PATH — {}",
                tool.program(),
                tool.install_hint()
            ),
            WorkFault::ToolFailed { tool, why } => {
                format!("`{}` would not start: {why}", tool.program())
            }
            WorkFault::NoDirectory { path, why } => {
                format!("{path} could not be made: {why}")
            }
            WorkFault::NoStore => "this platform has no data directory, so a module has \
                                   nowhere to be fetched into"
                .to_string(),
            WorkFault::Module(f) => f.sentence(),
            WorkFault::NoSource { producer } => format!(
                "nothing is approved as `{producer}`, so there is no repository to re-approve \
                 — name one with `from <url>`"
            ),
            WorkFault::CloneFailed { url, why } => format!(
                "{url} could not be cloned: {why}\n  a private repository needs a credential \
                 helper git can use without asking — this console never prompts for one"
            ),
            WorkFault::FetchFailed { url, reference, why } => format!(
                "{url} could not be fetched at `{reference}`: {why}"
            ),
            WorkFault::NoSuchCommit { url, reference } => format!(
                "`{reference}` names nothing in {url} — a branch that was deleted, a tag that \
                 moved, or a commit the remote will not serve directly"
            ),
            WorkFault::NotAHash { reference, answer } => format!(
                "git resolved `{reference}` to `{answer}`, which is not a commit hash — \
                 nothing was recorded, because an approval that does not name bytes names \
                 nothing"
            ),
            WorkFault::NoManifest { producer, commit } => format!(
                "{commit} has no {MANIFEST_FILE}, so it is not offering itself as a module — \
                 `{producer}` was not approved"
            ),
            WorkFault::IdentityMismatch { asked, found } => format!(
                "you named `{asked}` and the repository calls itself `{found}` — approve it \
                 under its own name, or check you named the repository you meant"
            ),
            WorkFault::NotApproved { producer, known } if known.is_empty() => format!(
                "nothing is approved as `{producer}`, and nothing is approved at all — \
                 `{}` approves a repository",
                crate::module::APPROVE_VERB
            ),
            WorkFault::NotApproved { producer, known } => format!(
                "nothing is approved as `{producer}` — approved: {}",
                known.join(", ")
            ),
            WorkFault::BuildFailed { producer, tail } => format!(
                "`{producer}` did not build:\n{tail}\n  the approval stands and nothing was \
                 recorded as built — fix it in the repository and build again"
            ),
            WorkFault::DiffFailed { producer, tail } => {
                format!("`{producer}` could not be compared:\n{tail}")
            }
            WorkFault::LaunchFailed { path, why } => {
                format!("{path} would not start: {why}")
            }
            WorkFault::NoBinary { producer, expected } => format!(
                "`{producer}` built, and produced no binary named `{producer}` — nothing is at \
                 {expected}. A module's repository must produce a release binary named for its \
                 producer from a plain `cargo build --release`; cargo SKIPS a `[[bin]]` whose \
                 `required-features` are off, silently and with exit 0, which is the usual cause"
            ),
            // ⚠️ **The sentence has to concede the exit code, or it reads as the console being
            // broken.** The person's first reaction is *"but it built fine"* — and it did; cargo
            // exited 0 and was honest, because it was asked to build a target it then had no
            // reason to build. A refusal that does not say so is one nobody believes.
            WorkFault::StaleBinary { producer, expected } => format!(
                "`{producer}` built successfully and produced no binary — cargo exited 0 and was \
                 honest, because it never built one. What is at {expected} is left over from an \
                 earlier build, so it is NOT the commit just checked out, and launching it would \
                 run bytes no record names. Usually a `[[bin]]` whose `required-features` are off. \
                 Delete it and build again to see the real failure"
            ),
            WorkFault::ChannelFailed { producer, why } => format!(
                "`{producer}` has nowhere to draw — its frame channel could not be created: \
                 {why}"
            ),
        }
    }
}

impl std::fmt::Display for WorkFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence())
    }
}

// ---------------------------------------------------------------------------------------
// The action word
// ---------------------------------------------------------------------------------------

/// Which of the five a `console module` line is asking for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleCmd {
    Approve,
    Build,
    Diff,
    Revoke,
    /// 🚨 **T5's, and the one that runs no program in this file.** Restarting is *stop the
    /// process and launch it again* — [`crate::module_host`], on the frame thread, in
    /// microseconds — so it takes `revoke`'s route rather than a job's: no network, no
    /// compiler, nothing to queue behind. The verb is here because
    /// [`crate::module::RESTART_VERB`] is printed inside a rectangle that a person then types
    /// at, and a sentence naming a word the grammar refuses is worse than no sentence.
    Restart,
}

impl ModuleCmd {
    /// Resolve the action word. **Exact, never approximated** —
    /// [`crate::module::ModuleRegistry::get`]'s rule, and here the distance between two of
    /// these words is the distance between granting trust and withdrawing it.
    pub fn resolve(word: &str) -> Result<Self, String> {
        match word {
            "approve" => Ok(ModuleCmd::Approve),
            "build" => Ok(ModuleCmd::Build),
            "diff" => Ok(ModuleCmd::Diff),
            "revoke" => Ok(ModuleCmd::Revoke),
            "restart" => Ok(ModuleCmd::Restart),
            other => Err(format!(
                "`{other}` is not something `console module` does — {}",
                MODULE_ACTIONS.join(", ")
            )),
        }
    }

    pub fn as_word(self) -> &'static str {
        match self {
            ModuleCmd::Approve => "approve",
            ModuleCmd::Build => "build",
            ModuleCmd::Diff => "diff",
            ModuleCmd::Revoke => "revoke",
            ModuleCmd::Restart => "restart",
        }
    }
}

// ---------------------------------------------------------------------------------------
// Where a checkout lives
// ---------------------------------------------------------------------------------------

/// `<store_root>/modules/<producer>` — where this module's bytes are, and where they stay.
///
/// 📌 **Approving the same repository twice reuses this directory rather than making a second
/// one.** The producer name is unique in `modules.json` by construction
/// ([`crate::module::ModuleRegistry::upsert`] replaces by producer), so one directory per
/// producer is not a convention that could be broken — it is the same key. What changes on a
/// re-approval is which commit is fetched into it, which is a `git fetch` rather than a second
/// clone of a repository already on the disk.
///
/// 🚨 **The producer name is checked before it becomes a path component**, and
/// [`crate::module::check_producer_name`] grew two rules for this call site: a name of `..`
/// satisfies every rule that was there when it was written and names the store root's parent.
pub fn checkout_dir(store_root: &Path, producer: &str) -> Result<PathBuf, WorkFault> {
    crate::module::check_producer_name(producer)?;
    Ok(store_root.join(MODULES_DIR).join(producer))
}

// ---------------------------------------------------------------------------------------
// approve
// ---------------------------------------------------------------------------------------

/// What a person chose to grant, as the approve line said it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    /// **No `grant` word at all — a dry run.** Fetch, read the manifest, say what it asks
    /// for, record nothing.
    ///
    /// 📌 `CLAUDE.md` invariant 4 where it matters most: a new capability defaults to inert,
    /// so the default of the verb that grants things is *"tell me what this wants"*. A
    /// mistyped approve therefore cannot grant anything, and the request is on screen before
    /// the answer is typed — which is §3.1's *"the manifest requests, the record grants"*
    /// arriving as a gesture rather than only as a type.
    Show,
    /// The names a person picked. Empty is [`NONE_GRANT`] — approve, grant nothing.
    Names(Vec<String>),
}

impl Grant {
    /// Read the word after `grant`. Absent is [`Grant::Show`]; that decision is the caller's,
    /// because only the caller knows whether the word was there.
    pub fn parse(word: &str) -> Grant {
        if word == NONE_GRANT {
            return Grant::Names(Vec::new());
        }
        Grant::Names(
            word.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        )
    }
}

/// What [`approve`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Approval {
    /// The dry run. **Nothing was recorded**, and the sentence says what would be.
    Requested {
        producer: String,
        name: String,
        url: String,
        commit: String,
        reference: Option<String>,
        requests: Requested,
    },
    /// An approval, ready for the frame thread to put in the registry and save.
    Recorded(Box<ApprovedModule>),
}

impl Approval {
    /// The line the console says.
    ///
    /// 🚨 [`BUILD_TRUST`] is in **both** arms. The dry run carries it because that is the
    /// moment a person is deciding; the recorded arm carries it because a person who typed the
    /// grant list straight out never saw the dry run.
    ///
    /// 🚨 **Everything here that came out of the manifest goes through
    /// [`crate::module::quoted_untrusted`]** — the display name and the requested grant names.
    /// This is the console speaking in its own voice about a file somebody else wrote, and a
    /// `name` carrying a newline could print a second `organon-console:` line saying whatever
    /// it liked, including that it had been granted something. `producer` is the exception and
    /// the reason is exact: it has already been through
    /// [`crate::module::check_producer_name`], which is why it is the one field a repository
    /// supplies that can be rendered bare.
    pub fn sentence(&self) -> String {
        match self {
            Approval::Requested { producer, name, url, commit, reference, requests } => {
                let asks = if requests.is_empty() {
                    "it asks for nothing".to_string()
                } else {
                    format!(
                        "it asks for {}",
                        requests
                            .names()
                            .map(crate::module::quoted_untrusted)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                // 🚨 **The suggested line is built from only the names that can TRAVEL**, and
                // this one is not quoted because it is meant to be copied and typed. A grant
                // name carrying whitespace, a control character or a comma cannot be expressed
                // on a whitespace-delimited wire inside a comma-separated list — so offering it
                // would be offering a line that grants something else, or nothing. The excluded
                // ones are named rather than dropped silently: a request this console cannot
                // grant is a true and useful thing to say.
                let (sayable, unsayable): (Vec<&str>, Vec<&str>) =
                    requests.names().partition(|n| wire_safe(n));
                let grant_words = if sayable.is_empty() {
                    NONE_GRANT.to_string()
                } else {
                    sayable.join(",")
                };
                let unsayable = if unsayable.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n  ⚠️ {} cannot be granted: a grant name crosses the console's \
                         channel inside a comma-separated list, so it may not contain \
                         whitespace, a comma or a control character.",
                        unsayable
                            .iter()
                            .map(|n| crate::module::quoted_untrusted(n))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!(
                    "`{producer}` ({}) is {url} at {}{} — {asks}.\n  \
                     NOTHING WAS RECORDED. `{} {producer} grant {NONE_GRANT}` approves it with \
                     nothing granted; `{} {producer} grant {grant_words}` grants what it asked \
                     for.{unsayable}\n  ⚠️ {BUILD_TRUST}.",
                    crate::module::quoted_untrusted(name),
                    short(commit),
                    reference.as_ref().map(|r| format!(" (from {r})")).unwrap_or_default(),
                    crate::module::APPROVE_VERB,
                    crate::module::APPROVE_VERB,
                )
            }
            Approval::Recorded(m) => {
                let granted = if m.granted.is_empty() {
                    "nothing granted".to_string()
                } else {
                    format!(
                        "granted {}",
                        m.granted
                            .names()
                            .map(crate::module::quoted_untrusted)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!(
                    "`{}` ({}) approved at {} — {granted}.\n  \
                     `{}` builds it.\n  ⚠️ {BUILD_TRUST}.",
                    m.producer,
                    crate::module::quoted_untrusted(&m.name),
                    m.short_commit(),
                    crate::module::BUILD_VERB,
                )
            }
        }
    }
}

/// **Approve a repository as the module `producer`.** §3.1, §3.2 and §3.4 in one function.
///
/// `url` is where the bytes live; `reference` is the branch, tag or commit to take — `None`
/// means the remote's own default. Neither comes from a manifest: both are typed by a person,
/// which is what stops a repository choosing where the console fetches from.
///
/// 🚨 **The commit is resolved by git before anything is recorded**, so §3.2's *"a reference
/// naming only a branch is a reference that has not decided what it trusts"* holds however the
/// line was typed. `at main` is legal and useful; what is stored is the forty-character answer
/// `main` had on the day, with `main` beside it as provenance
/// ([`ModuleSource::reference`]).
///
/// ⚠️ **This runs no part of the module.** A clone and a `git show` touch no `build.rs` and no
/// proc macro — see the header. [`build`] is the function that spends the trust this one asks
/// for.
pub fn approve(
    shop: &dyn Workshop,
    store_root: &Path,
    producer: &str,
    url: &str,
    reference: Option<&str>,
    grant: &Grant,
) -> Result<Approval, WorkFault> {
    let checkout = checkout_dir(store_root, producer)?;
    ensure_repo(shop, &checkout, url)?;
    let commit = fetch_and_resolve(shop, &checkout, url, reference)?;

    // 🚨 Read out of the object database at the **approved commit**, never off the working
    // tree. The manifest that matters is the one belonging to the bytes being approved; a
    // working tree can be anything, including whatever a previous build left checked out.
    let shown = shop.run(
        Tool::Git,
        &["show", &format!("{commit}:{MANIFEST_FILE}")],
        &checkout,
    )?;
    if !shown.succeeded() {
        return Err(WorkFault::NoManifest {
            producer: producer.to_string(),
            commit: short(&commit).to_string(),
        });
    }
    let manifest = ModuleManifest::parse(&shown.stdout)?;
    if manifest.producer != producer {
        return Err(WorkFault::IdentityMismatch {
            asked: producer.to_string(),
            found: manifest.producer,
        });
    }

    match grant {
        Grant::Show => Ok(Approval::Requested {
            producer: manifest.producer,
            name: manifest.name,
            url: url.to_string(),
            commit,
            reference: reference.map(str::to_string),
            requests: manifest.requests,
        }),
        Grant::Names(names) => {
            let source = ModuleSource {
                url: url.to_string(),
                commit,
                reference: reference.map(str::to_string),
            };
            let chosen: Vec<&str> = names.iter().map(String::as_str).collect();
            // 🚨 **`ApprovedModule::approve`, never a record assembled here.** It derives the
            // `Granted` from the manifest it is approving, in its own hand, so a grant that
            // answers a request nobody made of *this* module is unrepresentable rather than
            // merely unlikely. Routing around that is the one thing this tier was told not to
            // do, and there is no other constructor in this file.
            Ok(Approval::Recorded(Box::new(ApprovedModule::approve(&manifest, source, &chosen)?)))
        }
    }
}

// ---------------------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------------------

/// What [`build`] produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub producer: String,
    /// For the frame thread to hang on the record. Its `commit` is what `git rev-parse HEAD`
    /// answered **at the moment of the build**, not what was approved.
    pub record: BuildRecord,
    /// Where the artifacts went. See [`artifact_dir`].
    pub artifacts: PathBuf,
    /// What the approved commit was, so the sentence can say whether the two agree.
    pub approved_commit: String,
}

impl Built {
    /// ⚠️ **The one thing this sentence must never do is imply the build named the approved
    /// bytes when it did not.** [`BuildRecord::names_approved_bytes`] is the single comparison
    /// site and this reads it rather than comparing again.
    pub fn sentence(&self) -> String {
        if self.record.names_approved_bytes(&self.approved_commit) {
            return format!(
                "`{}` built at {} — {}",
                self.producer,
                short(&self.record.commit),
                self.artifacts.display()
            );
        }
        if self.record.dirty {
            return format!(
                "`{}` built from a DIRTY tree at {} — recorded as dirty, because no hash names \
                 the bytes that were compiled. {} still holds the picture: a viewport asking \
                 for it will say it is not built. Commit the checkout, or build the approved \
                 commit again.",
                self.producer,
                short(&self.record.commit),
                self.artifacts.display()
            );
        }
        format!(
            "`{}` built at {}, which is NOT the approved commit {} — the record now describes \
             a binary that is not the one approved. Approve {} to trust what was built.",
            self.producer,
            short(&self.record.commit),
            short(&self.approved_commit),
            short(&self.record.commit),
        )
    }
}

/// Where a built module's artifacts land: `<checkout>/target/release`.
///
/// 📌 **Derived, never recorded**, and that is the argument for it rather than a shortcut. A
/// stored path is a second statement of where the build went, and the two can come apart — a
/// store that moved, a checkout deleted by hand, a record written by an older console. A
/// function of the store root and the producer cannot: whatever asks, asks the same way.
pub fn artifact_dir(store_root: &Path, producer: &str) -> Result<PathBuf, WorkFault> {
    Ok(checkout_dir(store_root, producer)?.join("target").join("release"))
}

/// The file extension an executable carries here. `.exe` on Windows, nothing elsewhere.
///
/// ⚠️ `std::env::consts::EXE_SUFFIX` rather than a `cfg!` chain, so this is right on a platform
/// nobody has thought about yet — and it is the *host* suffix, which is correct because the
/// binary was built by this machine's `cargo` a moment ago.
const EXE_SUFFIX: &str = std::env::consts::EXE_SUFFIX;

/// **The binary a module is launched from** — `<checkout>/target/release/<producer>[.exe]`.
///
/// 🚨 **Derived, never recorded, and never named by the module.** [`artifact_dir`]'s argument
/// applies unchanged — a stored path is a second statement of where the build went and the two
/// can come apart — but here there is a second and stronger one. A `binary = ` key in
/// `organon-module.toml` would be a string, written by somebody else, arriving at
/// [`std::process::Command::new`]; this file's header forbids exactly that for [`Tool`] and the
/// rule does not weaken because the program is the module's own. The producer name is the only
/// influence a manifest has, it is a name the *person* typed and the repository agreed to
/// (`IdentityMismatch`), and [`crate::module::check_producer_name`] gates it before it is a path
/// component.
///
/// 📌 **So a module's cargo package must produce a binary named after its producer**, which is
/// the convention `ascent` already satisfies. It is a real constraint and it is the cheap end
/// of the trade: the alternative is reading a program name out of a file a person can edit.
pub fn module_binary(store_root: &Path, producer: &str) -> Result<PathBuf, WorkFault> {
    Ok(artifact_dir(store_root, producer)?.join(format!("{producer}{EXE_SUFFIX}")))
}

/// **Did this cargo run actually produce `binary`?**
///
/// 🚨 **The question `file_exists` cannot answer, and the whole of the staleness half.** Cargo
/// emits one `compiler-artifact` record per unit it built *or confirmed current*, each carrying
/// the `executable` it produced. A target cargo skipped emits **nothing at all** — so the absence
/// of a record naming our binary is cargo saying, in its own voice, that this build did not
/// produce it.
///
/// # ⚠️ Three candidate mechanisms, measured rather than argued
///
/// Built as a scratch crate with a `required-features` bin and run in all four states
/// (2026-08-22):
///
/// | | cargo exit | binary on disk | `compiler-artifact` | `file_exists` | mtime | this |
/// |---|---|---|---|---|---|---|
/// | skipped, nothing there before | 0 | no | none | refuses ✓ | refuses ✓ | refuses ✓ |
/// | a real build | 0 | yes | yes | passes ✓ | passes ✓ | passes ✓ |
/// | **an up-to-date rebuild** | 0 | yes | yes, `fresh` | passes ✓ | **REFUSES ✗** | passes ✓ |
/// | **a stale leftover, now skipped** | 0 | yes | none | **PASSES ✗** | refuses ✓ | refuses ✓ |
///
/// 🚨 **The mtime candidate — stamp the build's start, compare the artifact's — fails on the one
/// case that is ordinary.** Cargo does not relink when nothing changed, so a second build of the
/// same commit leaves an mtime older than the build that just succeeded. The first two rows are
/// misconfigurations; row three is Tuesday. A check that refuses an ordinary rebuild is withdrawn
/// within a day, **and takes the row-four hole with it** — so it does not merely fail, it fails in
/// the way that discredits the check it was meant to be.
///
/// 📌 **`fresh` is deliberately not consulted**, and it is the trap here because it *looks* like
/// exactly what a staleness check should gate on. A fresh artifact is cargo asserting the binary
/// on disk **is** current for these inputs — which is the question being asked, not a reason for
/// suspicion. Gating on it re-introduces row three.
///
/// ⚠️ **Matched on file NAME, not on the full path, and the error direction is why.** Cargo prints
/// an absolute path; a store root reached through a symlink, a junction or a short path yields a
/// different spelling of the same file, and an exact compare would then refuse a good build — row
/// three's failure by another road. The name is unambiguous within one package, and cargo is run
/// in the module's own checkout, so every artifact it reports belongs to that checkout.
///
/// 📌 **A malformed or unparseable line is skipped, never fatal.** Cargo's record set grows, and a
/// reason this build does not recognise is not evidence about our binary either way.
fn produced_executable(stdout: &str, binary: &Path) -> bool {
    let Some(want) = binary.file_name() else { return false };
    stdout.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).any(|m| {
        m.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact")
            && m.get("executable")
                .and_then(|e| e.as_str())
                .map(|e| Path::new(&e.replace('\\', "/")).file_name() == Some(want))
                .unwrap_or(false)
    })
}

/// **Build the approved commit**, and record what was actually built.
///
/// §3.4's corollary is the whole of this function's shape: *"`modules.json` records the commit
/// that was BUILT, not the commit that was approved, and they must be equal or the record
/// describes a binary that does not exist."* So the order is: check the approved commit out,
/// ask git what `HEAD` is **and** whether the tree is dirty, and only then compile — the two
/// facts are read as close to the compiler as they can be read.
///
/// ⚠️ **A dirty tree is recorded, not refused.** §3.4 again: *"if the tree was dirty at build
/// time, record that rather than recording a commit as though it named the bytes."* A person
/// hacking on a module in that checkout is a thing to support; a record quietly claiming those
/// bytes were a commit is not.
///
/// 🚨 This is where [`BUILD_TRUST`] is spent. Everything the repository's `build.rs` and
/// proc macros do, they do here.
pub fn build(
    shop: &dyn Workshop,
    store_root: &Path,
    module: &ApprovedModule,
) -> Result<Built, WorkFault> {
    let checkout = checkout_dir(store_root, &module.producer)?;
    ensure_repo(shop, &checkout, &module.url)?;
    // The approved commit may not be in this checkout — a store restored from a backup, or a
    // record approved on another machine — so fetch before checking out rather than after
    // failing to.
    if !has_commit(shop, &checkout, &module.commit)? {
        fetch_and_resolve(shop, &checkout, &module.url, Some(&module.commit))?;
    }
    let out = shop.run(Tool::Git, &["checkout", "--detach", &module.commit], &checkout)?;
    if !out.succeeded() {
        return Err(WorkFault::NoSuchCommit {
            url: module.url.clone(),
            reference: module.commit.clone(),
        });
    }

    let head = shop.run(Tool::Git, &["rev-parse", "HEAD"], &checkout)?;
    let commit = head.line().to_string();
    if !crate::module::is_commit_hash(&commit) {
        return Err(WorkFault::NotAHash { reference: "HEAD".into(), answer: commit });
    }
    // ⚠️ `--porcelain` because the human-readable form is localised and reflowed; this is the
    // one output git promises is stable. Any line at all means the tree is not the commit.
    let status = shop.run(Tool::Git, &["status", "--porcelain"], &checkout)?;
    let dirty = !status.stdout.trim().is_empty();

    // ⚠️ **`json-render-diagnostics`, NOT plain `json`, and the difference is what a person sees
    // when a build fails.** Both put machine-readable records on stdout; plain `json` *also* moves
    // the compiler's diagnostics there, as JSON, leaving stderr holding two lines — `Compiling…`
    // and `could not compile … due to 1 previous error`. [`ToolOutput::tail`] prefers stderr, so
    // [`WorkFault::BuildFailed`] would carry that summary and **not the error**. Measured on a
    // failing build: plain `json` leaves zero rendered diagnostics on stderr, this leaves the
    // whole thing. It also makes stdout pure JSON, which is what [`produced_executable`] parses.
    let built = shop.run(
        Tool::Cargo,
        &["build", "--release", "--message-format=json-render-diagnostics"],
        &checkout,
    )?;
    if !built.succeeded() {
        return Err(WorkFault::BuildFailed {
            producer: module.producer.clone(),
            tail: built.tail(),
        });
    }

    // 🚨 **An exit code that is true about the command and false about the outcome.**
    //
    // **Cargo silently skips a `[[bin]]` whose `required-features` are not enabled** — no
    // warning, no diagnostic, exit 0 in 0.12 s. So `succeeded()` above is `true` for a build
    // that produced nothing, and without this check the console records a successful build of a
    // commit whose binary does not exist. The failure then surfaces **two layers away** — as a
    // missing file at launch, or as §4.6's *"launched, not yet producing"* timing out after ten
    // seconds — with nothing anywhere naming the cause.
    //
    // ⚠️ **Not a workaround for one module.** Ascent is where it was found (`ascent#86`, measured
    // in a clean tree: `rm target/release/ascent.exe`, `cargo build --release`, exit 0, file
    // still absent), but the class is every module there will ever be: a `required-features`
    // bin, a workspace default-member quirk, a `[[bin]]` typo. One sentence at the moment the
    // fact is knowable, which is the whole posture of §4.6.
    //
    // 📌 It is fair to refuse rather than arbitrary **because the obligation is published**:
    // `doc/organon_module_viewport.md` §4.7 says a module's repository must produce a release
    // binary named for its producer from a plain `cargo build --release`. This is that
    // requirement being checked, not a preference being enforced.
    let binary = module_binary(store_root, &module.producer)?;
    if !shop.file_exists(&binary) {
        return Err(WorkFault::NoBinary {
            producer: module.producer.clone(),
            expected: binary.display().to_string(),
        });
    }
    // 🚨 **…and the half `file_exists` cannot see.** A binary left by an earlier build satisfies
    // the check above while belonging to a different commit, after which the record names bytes
    // that are not the ones on disk and the console launches them. See [`produced_executable`] for
    // the four measured cases and for why this is not an mtime comparison.
    //
    // ⚠️ **Second, not first**, so the two refusals land in the order a person can act on: with
    // nothing on disk at all, *"the repository produces no such binary"* is the more useful
    // sentence, and it is [`WorkFault::NoBinary`]'s.
    if !produced_executable(&built.stdout, &binary) {
        return Err(WorkFault::StaleBinary {
            producer: module.producer.clone(),
            expected: binary.display().to_string(),
        });
    }

    Ok(Built {
        producer: module.producer.clone(),
        record: BuildRecord { commit, dirty, extra: BTreeMap::new() },
        artifacts: artifact_dir(store_root, &module.producer)?,
        approved_commit: module.commit.clone(),
    })
}

// ---------------------------------------------------------------------------------------
// diff — 📌 the verb the whole approach is git-based FOR
// ---------------------------------------------------------------------------------------

/// One changed path, as `git diff --name-status` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// `M`, `A`, `D`, `R100`… — carried whole rather than interpreted, because git's set grows
    /// and a status this console does not recognise is still a true thing to show a person.
    pub status: String,
    pub path: String,
}

/// What [`compare`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub producer: String,
    pub url: String,
    pub approved: String,
    pub candidate: String,
    pub reference: Option<String>,
    pub changes: Vec<Change>,
}

impl Comparison {
    pub fn unchanged(&self) -> bool {
        self.approved == self.candidate
    }

    /// 🚨 §3.3, said the way §3.3 says it: *"This module has changed 14 files since the commit
    /// you last trusted. Here they are."* — and then the verb that asks again.
    ///
    /// The re-approval line is **spelled out with the candidate commit in it**, so renewing
    /// trust is one line a person can read and copy rather than a verb they have to reassemble.
    /// That is the whole affordance: `git diff <approved>..<candidate>` is one command and no
    /// package manager offers it, but a diff nobody can act on is a report rather than a gate.
    pub fn sentence(&self) -> String {
        if self.unchanged() {
            return format!(
                "`{}` is unchanged since you approved it — still {}",
                self.producer,
                short(&self.approved)
            );
        }
        let n = self.changes.len();
        let listed: Vec<String> = self
            .changes
            .iter()
            .take(MAX_LISTED)
            .map(|c| format!("    {} {}", c.status, c.path))
            .collect();
        let more = n.saturating_sub(listed.len());
        format!(
            "`{}` has changed {n} file{} since the commit you last trusted ({} → {}{}). Here \
             they are:\n{}{}\n  `{} {} from {} at {} grant …` trusts the new one — and \
             nothing changes until you do.",
            self.producer,
            if n == 1 { "" } else { "s" },
            short(&self.approved),
            short(&self.candidate),
            self.reference.as_ref().map(|r| format!(", from {r}")).unwrap_or_default(),
            listed.join("\n"),
            if more == 0 { String::new() } else { format!("\n    … and {more} more") },
            crate::module::APPROVE_VERB,
            self.producer,
            self.url,
            self.candidate,
        )
    }
}

/// **Show what changed since the approved commit, and ask again.**
///
/// §11.4 is the reason this exists and §3.3 is the reason it is the most valuable of the four:
/// trust is not granted once, it is renewed at every update, and *"the code you audited is not
/// the code that arrived"*. `git diff <approved>..<candidate>` is one command, and no package
/// manager offers it.
///
/// 🚨 **This changes nothing.** It fetches, it compares, it prints. The approval stands at
/// whatever commit it stood at; `modules.json` is not touched; no build runs. The verb is not
/// "update" and must not become one — the person, having read the list, types the approve line
/// or does not.
pub fn compare(
    shop: &dyn Workshop,
    store_root: &Path,
    module: &ApprovedModule,
    reference: Option<&str>,
) -> Result<Comparison, WorkFault> {
    let checkout = checkout_dir(store_root, &module.producer)?;
    ensure_repo(shop, &checkout, &module.url)?;
    // No `reference` means "whatever this repository's default branch is now", which is the
    // question a person asking *has it changed?* is actually asking. The recorded reference is
    // provenance rather than a subscription, so it is deliberately not used as the default:
    // approving a tag once does not mean asking about that tag forever.
    let candidate = fetch_and_resolve(shop, &checkout, &module.url, reference)?;
    if candidate == module.commit {
        return Ok(Comparison {
            producer: module.producer.clone(),
            url: module.url.clone(),
            approved: module.commit.clone(),
            candidate,
            reference: reference.map(str::to_string),
            changes: Vec::new(),
        });
    }
    let range = format!("{}..{}", module.commit, candidate);
    let out = shop.run(Tool::Git, &["diff", "--name-status", &range], &checkout)?;
    if !out.succeeded() {
        return Err(WorkFault::DiffFailed { producer: module.producer.clone(), tail: out.tail() });
    }
    Ok(Comparison {
        producer: module.producer.clone(),
        url: module.url.clone(),
        approved: module.commit.clone(),
        candidate,
        reference: reference.map(str::to_string),
        changes: parse_name_status(&out.stdout),
    })
}

/// `git diff --name-status`'s output, one `Change` per line.
///
/// ⚠️ **Split on the FIRST tab only.** A rename is `R100\told\tnew`, and a path may itself
/// contain almost anything; taking the remainder whole means an odd path is shown oddly rather
/// than being silently truncated to its first component. A line with no tab is not a change
/// and is dropped.
fn parse_name_status(text: &str) -> Vec<Change> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let (status, path) = line.split_once('\t')?;
            if status.is_empty() || path.is_empty() {
                return None;
            }
            Some(Change { status: status.to_string(), path: path.to_string() })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------
// The git steps the three verbs share
// ---------------------------------------------------------------------------------------

/// Make sure `checkout` is a git repository of `url`, cloning it if it is not one yet.
///
/// 📌 **The "approved twice" answer.** A directory that is already a repository is kept and
/// fetched into; only an absent one is cloned. So re-approving Ascent at a newer commit costs
/// a fetch, and the objects behind the commit a person is comparing *against* are still there
/// — which is what makes [`compare`] a local operation once the fetch is done.
fn ensure_repo(shop: &dyn Workshop, checkout: &Path, url: &str) -> Result<(), WorkFault> {
    shop.ensure_dir(checkout)?;
    // Asking git rather than looking for a `.git` entry: a worktree's `.git` is a file, a
    // submodule's is a file, and the question that matters is the one git will answer when the
    // next command runs.
    let probe = shop.run(Tool::Git, &["rev-parse", "--git-dir"], checkout)?;
    if probe.succeeded() {
        return Ok(());
    }
    // `.` because the directory already exists — `git clone <url> <dir>` refuses a non-empty
    // destination, and `ensure_dir` just made this one.
    let out = shop.run(Tool::Git, &["clone", url, "."], checkout)?;
    if out.succeeded() {
        Ok(())
    } else {
        Err(WorkFault::CloneFailed { url: url.to_string(), why: out.tail() })
    }
}

/// Is this commit already in the checkout's object database?
fn has_commit(shop: &dyn Workshop, checkout: &Path, commit: &str) -> Result<bool, WorkFault> {
    // `^{commit}` rather than a bare rev-parse: a bare one succeeds for any 40 hex characters
    // whether or not the object is there, so it would answer "yes" about bytes nobody has.
    let out =
        shop.run(Tool::Git, &["rev-parse", "--verify", &format!("{commit}^{{commit}}")], checkout)?;
    Ok(out.succeeded())
}

/// Fetch `reference` from `url` and turn it into **one commit hash**.
///
/// 🚨 §3.2 lives here. Whatever a person typed — a branch, a tag, a hash, or nothing at all —
/// what comes back is forty characters, and that is what any record gets. A fetch that left a
/// branch name in the record would be the exact failure T3a refuses on load, created by the
/// code that was supposed to prevent it.
///
/// ⚠️ **Two fetches, because a server may refuse to serve a bare hash.** `uploadpack.
/// allowReachableSHA1InWant` is off by default on most hosts, so `git fetch <url> <sha>` fails
/// against them while the object is perfectly reachable from the default branch. The fallback
/// is to fetch everything the remote offers and resolve locally — which is slower and always
/// works. Ordered this way round because the direct fetch is the cheap case and is what a
/// commit outside the default branch's history needs.
fn fetch_and_resolve(
    shop: &dyn Workshop,
    checkout: &Path,
    url: &str,
    reference: Option<&str>,
) -> Result<String, WorkFault> {
    let named = reference.unwrap_or("HEAD");
    let direct = shop.run(Tool::Git, &["fetch", "--tags", "--force", url, named], checkout)?;
    if direct.succeeded() {
        // FETCH_HEAD is what the fetch just wrote, whatever `named` was — which is why this
        // works identically for a branch, a tag and a hash the remote agreed to serve.
        if let Some(hash) = resolve(shop, checkout, "FETCH_HEAD")? {
            return Ok(hash);
        }
    }
    let all = shop.run(Tool::Git, &["fetch", "--tags", "--force", url], checkout)?;
    if !all.succeeded() {
        // The direct fetch's message is the more specific of the two when a reference was
        // named, and the general one is all there is when it was not.
        let why = if reference.is_some() { direct.tail() } else { all.tail() };
        return Err(WorkFault::FetchFailed {
            url: url.to_string(),
            reference: named.to_string(),
            why,
        });
    }
    // After a full fetch the reference is resolved locally. `FETCH_HEAD` again when nothing
    // was named, because that is where the remote's own default landed.
    let local = if reference.is_some() { named } else { "FETCH_HEAD" };
    resolve(shop, checkout, local)?
        .ok_or_else(|| WorkFault::NoSuchCommit {
            url: url.to_string(),
            reference: named.to_string(),
        })
}

/// `git rev-parse <what>^{commit}`, checked to be a hash. `None` if it names nothing.
fn resolve(shop: &dyn Workshop, checkout: &Path, what: &str) -> Result<Option<String>, WorkFault> {
    let out =
        shop.run(Tool::Git, &["rev-parse", "--verify", &format!("{what}^{{commit}}")], checkout)?;
    if !out.succeeded() {
        return Ok(None);
    }
    let answer = out.line().to_string();
    if crate::module::is_commit_hash(&answer) {
        Ok(Some(answer))
    } else {
        // A belt, and not a redundant one: this is the single place a value on its way into
        // `modules.json` is taken from another program's stdout.
        Err(WorkFault::NotAHash { reference: what.to_string(), answer })
    }
}

/// Can this grant name be typed on a `console module … grant a,b` line and arrive whole?
///
/// Three ways it cannot, each a fact about the transport rather than a taste: the sidecar line
/// is whitespace-delimited, the grant list is comma-separated, and a control character corrupts
/// the line it travels on. A manifest may request a name failing all three — nothing validates
/// what a repository asks for — and the honest answer is to say the request cannot be granted
/// rather than to offer a line that would grant something else.
fn wire_safe(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|c| c.is_whitespace() || c.is_control() || c == ',')
}

/// A commit cut for a sentence. [`ApprovedModule::short_commit`]'s length and its reason;
/// a free function because these sentences are said about commits no record holds yet.
fn short(commit: &str) -> &str {
    let n = commit.len().min(12);
    commit.get(..n).unwrap_or(commit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const APPROVED: &str = "1111111111111111111111111111111111111111";
    const CANDIDATE: &str = "2222222222222222222222222222222222222222";
    const URL: &str = "https://github.com/organonart/ascent";

    const MANIFEST: &str = r#"
        producer = "ascent"
        name = "Ascent"
        kind = "viewport"
        requests = ["audio", "input"]
    "#;

    /// A [`Workshop`] that runs nothing.
    ///
    /// 🚨 **Matched by argument prefix, and an unmatched call PANICS naming itself.** A fake
    /// that answered a default would let a test pass while the code under it ran a command
    /// nobody scripted — which is precisely the class of bug an injectable seam exists to
    /// catch. The panic message is the command, so a test that forgot a step says which.
    struct Fake {
        answers: Vec<(String, ToolOutput)>,
        calls: Mutex<Vec<String>>,
        dirs: Mutex<Vec<PathBuf>>,
        /// The paths this fake claims exist on disk — see `file_exists`. Empty by default, so a
        /// test that wants `build` to succeed has to SAY the binary was produced, which is the
        /// fact Ascent found cargo will silently not deliver.
        produced: Mutex<Vec<PathBuf>>,
    }

    impl Fake {
        /// ⚠️ **Prefixes are tried in order**, so a longer, more specific one must be listed
        /// above the general one it extends — `git fetch <url> <sha>` above `git fetch`.
        fn new(answers: Vec<(&str, ToolOutput)>) -> Self {
            Fake {
                answers: answers.into_iter().map(|(p, o)| (p.to_string(), o)).collect(),
                calls: Mutex::new(Vec::new()),
                dirs: Mutex::new(Vec::new()),
                produced: Mutex::new(Vec::new()),
            }
        }

        /// Every command run, as `git fetch --tags …`, in order.
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn ran(&self, needle: &str) -> bool {
            self.calls().iter().any(|c| c.contains(needle))
        }

        /// Every directory the job asked to exist.
        fn dirs(&self) -> Vec<PathBuf> {
            self.dirs.lock().unwrap().clone()
        }

        /// Say that the build really did produce this producer's binary.
        fn produces(self, store_root: &Path, producer: &str) -> Self {
            self.produced.lock().unwrap().push(module_binary(store_root, producer).unwrap());
            self
        }
    }

    impl Workshop for Fake {
        fn run(&self, tool: Tool, args: &[&str], _cwd: &Path) -> Result<ToolOutput, WorkFault> {
            let line = format!("{} {}", tool.program(), args.join(" "));
            self.calls.lock().unwrap().push(line.clone());
            for (prefix, out) in &self.answers {
                if line.starts_with(prefix.as_str()) {
                    return Ok(out.clone());
                }
            }
            panic!("nothing scripted for `{line}`");
        }

        fn ensure_dir(&self, path: &Path) -> Result<(), WorkFault> {
            self.dirs.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }

        /// ⚠️ **A scripted answer, and by default `false`.** The `Fake` builds against a
        /// `/store` that has never existed on any disk, so asking the real filesystem here would
        /// make every build test depend on the machine. `produces` is what a test says when it
        /// wants the build to have worked.
        fn file_exists(&self, path: &Path) -> bool {
            self.produced.lock().unwrap().iter().any(|p| p == path)
        }

        /// 🚨 **A panic, not a stub returning something inert.** Not one verb in this file
        /// launches anything — `approve`, `build`, `diff` and `revoke` run `git` and `cargo`
        /// and wait for them — so a `spawn` reached from a job is a job that has grown a
        /// capability it must not have. A `Ok(never-exits)` here would let that land silently
        /// and be discovered as a stray process; this fails the test that did it, by name.
        fn spawn(
            &self,
            program: &Path,
            _cwd: &Path,
            _env: &[(&str, String)],
        ) -> Result<Box<dyn ModuleProcess>, WorkFault> {
            panic!("a `module_work` verb tried to launch {} — none of them may", program.display());
        }
    }

    /// The happy path's script: an existing checkout, a fetch that resolves, a manifest.
    fn shop_with(commit: &str, manifest: &str) -> Fake {
        Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            ("git fetch", ToolOutput::ok("")),
            ("git rev-parse --verify FETCH_HEAD", ToolOutput::ok(commit)),
            ("git show", ToolOutput::ok(manifest)),
        ])
    }

    fn store() -> PathBuf {
        PathBuf::from("/store")
    }

    fn approved() -> ApprovedModule {
        ApprovedModule {
            producer: "ascent".into(),
            name: "Ascent".into(),
            url: URL.into(),
            commit: APPROVED.into(),
            reference: Some("main".into()),
            granted: Default::default(),
            built: None,
            settings: Vec::new(),
            extra: BTreeMap::new(),
        }
    }

    // -----------------------------------------------------------------------------------
    // §3.4 — the trust boundary
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **the build-time cost is in the sentence a person reads while approving**,
    /// in both spellings of approving — the dry run and the recorded one.
    ///
    /// §3.4 forbids letting *"the process is the boundary"* be believed further than it is
    /// true, and the only place that can be prevented is where the decision is made.
    #[test]
    fn approving_says_what_building_will_cost() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let shown =
            approve(&shop, &store(), "ascent", URL, None, &Grant::Show).unwrap().sentence();
        assert!(shown.contains(BUILD_TRUST), "the dry run must say it: {shown}");

        let recorded = approve(&shop, &store(), "ascent", URL, None, &Grant::Names(vec![]))
            .unwrap()
            .sentence();
        assert!(recorded.contains(BUILD_TRUST), "so must the recorded one: {recorded}");
    }

    /// CONTRACT: **approving runs no part of the module.** A clone and a `git show` execute
    /// nothing the repository wrote; `cargo` is reached from [`build`] alone.
    #[test]
    fn approving_never_runs_cargo() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        approve(&shop, &store(), "ascent", URL, None, &Grant::Names(vec!["audio".into()]))
            .unwrap();
        assert!(
            !shop.calls().iter().any(|c| c.starts_with("cargo")),
            "approve ran cargo: {:?}",
            shop.calls()
        );
    }

    /// CONTRACT: **no grant word grants nothing, and records nothing.** `CLAUDE.md`
    /// invariant 4 on the verb where it matters most — a mistyped approve cannot grant.
    #[test]
    fn a_grantless_approve_records_nothing() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let out = approve(&shop, &store(), "ascent", URL, None, &Grant::Show).unwrap();
        match &out {
            Approval::Requested { requests, .. } => {
                assert!(requests.contains("audio"), "the request is shown");
            }
            Approval::Recorded(_) => panic!("a dry run recorded an approval"),
        }
        assert!(out.sentence().contains("NOTHING WAS RECORDED"), "{}", out.sentence());
    }

    /// CONTRACT: **`grant none` approves and grants nothing** — which is a different thing
    /// from not approving, and both have to be sayable.
    #[test]
    fn grant_none_approves_with_nothing_granted() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let out =
            approve(&shop, &store(), "ascent", URL, None, &Grant::parse(NONE_GRANT)).unwrap();
        match out {
            Approval::Recorded(m) => {
                assert!(m.granted.is_empty(), "granted {:?}", m.granted);
                assert_eq!(m.commit, CANDIDATE);
            }
            Approval::Requested { .. } => panic!("`grant none` is an approval"),
        }
    }

    /// CONTRACT: **a grant is derived from the manifest being approved**, so a name that
    /// module never requested is refused rather than granted.
    #[test]
    fn a_grant_that_was_never_requested_is_refused() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let err = approve(&shop, &store(), "ascent", URL, None, &Grant::parse("network"))
            .unwrap_err();
        assert_eq!(
            err,
            WorkFault::Module(ModuleFault::NotRequested { grant: "network".into() })
        );
    }

    /// CONTRACT: **the repository does not get to be filed under a name it did not choose.**
    #[test]
    fn a_repository_calling_itself_something_else_is_refused() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let err = approve(&shop, &store(), "descent", URL, None, &Grant::Show).unwrap_err();
        assert_eq!(
            err,
            WorkFault::IdentityMismatch { asked: "descent".into(), found: "ascent".into() }
        );
    }

    // -----------------------------------------------------------------------------------
    // §3.2 — the unit is a commit
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **a reference naming only a branch never reaches the record.** `at main` is
    /// a perfectly good thing to type; what is stored is the hash `main` resolved to, with
    /// `main` beside it as provenance.
    #[test]
    fn a_branch_is_resolved_to_a_commit_before_anything_is_recorded() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        let out = approve(&shop, &store(), "ascent", URL, Some("main"), &Grant::parse(NONE_GRANT))
            .unwrap();
        match out {
            Approval::Recorded(m) => {
                assert_eq!(m.commit, CANDIDATE, "the hash, not the branch");
                assert_eq!(m.reference.as_deref(), Some("main"), "the branch is provenance");
            }
            Approval::Requested { .. } => unreachable!(),
        }
    }

    /// CONTRACT: **git's answer is checked before it becomes a record.** A `rev-parse` that
    /// returns something other than a hash is refused by name rather than stored.
    #[test]
    fn a_resolution_that_is_not_a_hash_is_refused() {
        let shop = Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            ("git fetch", ToolOutput::ok("")),
            ("git rev-parse --verify FETCH_HEAD", ToolOutput::ok("main")),
        ]);
        let err = approve(&shop, &store(), "ascent", URL, None, &Grant::Show).unwrap_err();
        assert!(matches!(err, WorkFault::NotAHash { .. }), "{err:?}");
        assert!(err.sentence().contains("nothing was recorded"), "{}", err.sentence());
    }

    /// CONTRACT: **a bare hash the remote will not serve still resolves**, through the full
    /// fetch. The direct fetch is tried first because it is the cheap case.
    #[test]
    fn a_refused_direct_fetch_falls_back_to_a_full_one() {
        let shop = Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            (
                "git fetch --tags --force https://github.com/organonart/ascent 2222",
                ToolOutput::failed("error: Server does not allow request for unadvertised object"),
            ),
            ("git fetch", ToolOutput::ok("")),
            ("git rev-parse --verify 2222", ToolOutput::ok(CANDIDATE)),
            ("git show", ToolOutput::ok(MANIFEST)),
        ]);
        let out = approve(&shop, &store(), "ascent", URL, Some(CANDIDATE), &Grant::Show).unwrap();
        assert!(matches!(out, Approval::Requested { .. }));
        assert_eq!(
            shop.calls().iter().filter(|c| c.starts_with("git fetch")).count(),
            2,
            "the direct fetch, then the full one: {:?}",
            shop.calls()
        );
    }

    // -----------------------------------------------------------------------------------
    // The checkout
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **a producer name cannot escape the store root.** The name comes out of a
    /// manifest and is used as a directory component; `..` satisfies every rule T3a wrote.
    #[test]
    fn a_producer_name_cannot_climb_out_of_the_store() {
        for hostile in ["..", ".", "../../etc", "a/b", "a\\b", "C:x"] {
            let err = checkout_dir(Path::new("/store"), hostile).unwrap_err();
            assert!(
                matches!(err, WorkFault::Module(ModuleFault::BadProducer { .. })),
                "`{hostile}` was accepted as a directory: {err:?}"
            );
        }
        assert_eq!(
            checkout_dir(Path::new("/store"), "ascent").unwrap(),
            Path::new("/store").join(MODULES_DIR).join("ascent")
        );
    }

    /// CONTRACT: **approving the same repository twice fetches rather than cloning.** One
    /// directory per producer, because the producer name is already the registry's key.
    #[test]
    fn a_second_approval_reuses_the_checkout() {
        let shop = shop_with(CANDIDATE, MANIFEST);
        approve(&shop, &store(), "ascent", URL, None, &Grant::Show).unwrap();
        assert!(!shop.ran("git clone"), "cloned over an existing checkout: {:?}", shop.calls());
        assert_eq!(
            shop.dirs(),
            vec![store().join(MODULES_DIR).join("ascent")],
            "one directory, keyed by the producer — which is already the registry's key"
        );

        let fresh = Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::failed("not a git repository")),
            ("git clone", ToolOutput::ok("")),
            ("git fetch", ToolOutput::ok("")),
            ("git rev-parse --verify FETCH_HEAD", ToolOutput::ok(CANDIDATE)),
            ("git show", ToolOutput::ok(MANIFEST)),
        ]);
        approve(&fresh, &store(), "ascent", URL, None, &Grant::Show).unwrap();
        assert!(fresh.ran("git clone"), "an empty directory must be cloned: {:?}", fresh.calls());
    }

    // -----------------------------------------------------------------------------------
    // build — §3.4's corollary
    // -----------------------------------------------------------------------------------

    /// What cargo's stdout looks like when it really did produce this producer's binary.
    ///
    /// 🚨 **The twin of [`Fake::produces`], and they are deliberately two statements rather than
    /// one.** `produces` answers `file_exists` — *is a binary there?* This answers
    /// [`produced_executable`] — *did THIS build make it?* A successful build says yes to both; the
    /// whole of case D is a `Fake` that says yes to the first and no to the second, and collapsing
    /// them into one switch would make that case unrepresentable in a test.
    ///
    /// ⚠️ **The path is derived from [`module_binary`] rather than written out**, so the `.exe`
    /// suffix is whatever this platform's build would really emit. A hand-typed `"ascent"` would
    /// pass on Linux and silently stop matching on Windows.
    fn cargo_built(producer: &str) -> ToolOutput {
        let exe = module_binary(&store(), producer).unwrap();
        ToolOutput::ok(&format!(
            "{{\"reason\":\"compiler-artifact\",\"executable\":{}}}\n\
             {{\"reason\":\"build-finished\",\"success\":true}}",
            serde_json::Value::from(exe.display().to_string())
        ))
    }

    /// Cargo exited 0 and built nothing — a `[[bin]]` whose `required-features` are off. Measured
    /// against a real scratch crate on 2026-08-22: the whole of stdout is one `build-finished`.
    fn cargo_built_nothing() -> ToolOutput {
        ToolOutput::ok("{\"reason\":\"build-finished\",\"success\":true}")
    }

    fn build_shop(head: &str, status: &str, cargo: ToolOutput) -> Fake {
        Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            ("git rev-parse --verify 1111", ToolOutput::ok(APPROVED)),
            ("git checkout --detach", ToolOutput::ok("")),
            ("git rev-parse HEAD", ToolOutput::ok(head)),
            ("git status --porcelain", ToolOutput::ok(status)),
            ("cargo build --release", cargo),
        ])
        // ⚠️ Every caller of this helper is testing a build that WORKED, so the helper says the
        // binary was produced. `a_build_that_produces_no_binary_is_not_a_build` is the one that
        // does not, and it builds its `Fake` by hand for exactly that reason.
        .produces(&store(), "ascent")
    }

    /// 🚨 **CONTRACT: a binary this build did not produce is refused — case D, the one
    /// `file_exists` cannot see.**
    ///
    /// Cargo exits 0, a binary from an earlier commit is still in `target/`, so the existence
    /// check passes. Without this the console records a built commit and then launches bytes no
    /// record names, with every indicator green.
    ///
    /// ⚠️ **Mutation-tested.** Deleting the `produced_executable` guard makes this return
    /// `Ok(Built { .. })` — a recorded build of a commit whose binary is a different commit's.
    #[test]
    fn a_binary_this_build_did_not_produce_is_refused() {
        let shop = build_shop(APPROVED, "", cargo_built_nothing());
        match build(&shop, &store(), &approved()) {
            Err(WorkFault::StaleBinary { producer, expected }) => {
                assert_eq!(producer, "ascent");
                let s = WorkFault::StaleBinary { producer, expected }.sentence();
                // The sentence must concede the exit code, or the person disbelieves it.
                assert!(s.contains("exited 0"), "{s}");
                assert!(s.contains("required-features"), "{s}");
            }
            other => panic!("a stale binary must not be recorded as a build: {other:?}"),
        }
    }

    /// 🚨 **CONTRACT: the two refusals are different sentences, because they are different
    /// diagnoses.** *No binary* is a fact about the repository; *a stale binary* is a fact about
    /// the checkout. A person's next action differs, so folding them together would make one of
    /// the two unavailable.
    #[test]
    fn nothing_built_and_something_stale_do_not_share_a_sentence() {
        let none = WorkFault::NoBinary { producer: "ascent".into(), expected: "/x".into() };
        let stale = WorkFault::StaleBinary { producer: "ascent".into(), expected: "/x".into() };
        assert_ne!(none.sentence(), stale.sentence());
        for f in [&none, &stale] {
            assert!(f.sentence().contains("ascent"));
            assert!(!f.sentence().contains('\n'), "one line: {}", f.sentence());
        }
    }

    /// 🚨 **CONTRACT: an up-to-date rebuild is NOT refused — case C, and the reason the mtime
    /// design was rejected.**
    ///
    /// Cargo relinks nothing when nothing changed, so the binary is older than the build that just
    /// succeeded — but it still reports the artifact, with `"fresh": true`. Measured on a real
    /// scratch crate. ⚠️ `fresh` is deliberately not consulted: it is cargo asserting the on-disk
    /// binary **is** current for these inputs, which is the question being asked. Gating on it
    /// would refuse every repeat build — the common case, and the failure that gets a check
    /// withdrawn and takes the case-D hole with it.
    #[test]
    fn an_up_to_date_rebuild_is_a_build() {
        let exe = module_binary(&store(), "ascent").unwrap();
        let fresh = ToolOutput::ok(&format!(
            "{{\"reason\":\"compiler-artifact\",\"fresh\":true,\"executable\":{}}}",
            serde_json::Value::from(exe.display().to_string())
        ));
        assert!(produced_executable(&fresh.stdout, &exe), "a fresh artifact is still an artifact");
        let shop = build_shop(APPROVED, "", fresh);
        assert!(build(&shop, &store(), &approved()).is_ok());
    }

    /// The parser itself, over the shapes cargo really emits and the ones it never does.
    #[test]
    fn only_an_artifact_naming_our_binary_counts() {
        let exe = module_binary(&store(), "ascent").unwrap();
        let art = |path: &str| {
            format!("{{\"reason\":\"compiler-artifact\",\"executable\":\"{path}\"}}")
        };

        assert!(produced_executable(&art(&exe.display().to_string().replace('\\', "/")), &exe));
        // 📌 Windows: cargo escapes its separators in JSON, and the parser normalises them.
        assert!(produced_executable(
            &art(&exe.display().to_string().replace('\\', "\\\\")),
            &exe
        ));
        // A different binary in the same workspace is not ours.
        assert!(!produced_executable(&art("/anywhere/somethingelse"), &exe));
        // The two cases that emit no artifact at all.
        assert!(!produced_executable("{\"reason\":\"build-finished\",\"success\":true}", &exe));
        assert!(!produced_executable("", &exe));
        // ⚠️ A record with no `executable` — a library unit — is not evidence of a binary.
        assert!(!produced_executable("{\"reason\":\"compiler-artifact\"}", &exe));
        // ⚠️ Unparseable lines are skipped, never fatal: cargo's record set grows, and a reason
        // this build does not know is not evidence either way.
        let noisy = format!("not json at all\n{{oops\n{}", art(&exe.display().to_string().replace('\\', "/")));
        assert!(produced_executable(&noisy, &exe));
    }

    /// CONTRACT: **the record names the commit that was BUILT.** A clean build of the
    /// approved commit is the case where the two agree, and the record says so through the
    /// one comparison site.
    #[test]
    fn a_clean_build_names_the_approved_bytes() {
        let shop = build_shop(APPROVED, "", cargo_built("ascent"));
        let out = build(&shop, &store(), &approved()).unwrap();
        assert_eq!(out.record.commit, APPROVED);
        assert!(!out.record.dirty);
        assert!(out.record.names_approved_bytes(APPROVED));
        assert!(out.sentence().contains("built at 111111111111"), "{}", out.sentence());
    }

    /// CONTRACT: **a dirty tree is recorded, never recorded as a commit.** §3.4: no hash
    /// names bytes that included uncommitted changes, so the record must not pretend one does
    /// — and the sentence must say the module is still not built.
    #[test]
    fn a_dirty_tree_is_recorded_as_dirty() {
        let shop = build_shop(APPROVED, " M src/main.rs\n", cargo_built("ascent"));
        let out = build(&shop, &store(), &approved()).unwrap();
        assert!(out.record.dirty);
        assert!(
            !out.record.names_approved_bytes(APPROVED),
            "a dirty build must not claim the approved bytes"
        );
        assert!(out.sentence().contains("DIRTY"), "{}", out.sentence());
    }

    /// CONTRACT: **the two commits are read as close to the compiler as they can be.** If
    /// `HEAD` is not the approved commit — a checkout somebody moved by hand between the
    /// fetch and the build — the record says what was built and the sentence says the two
    /// disagree.
    #[test]
    fn a_build_at_another_commit_says_so() {
        let shop = build_shop(CANDIDATE, "", cargo_built("ascent"));
        let out = build(&shop, &store(), &approved()).unwrap();
        assert_eq!(out.record.commit, CANDIDATE);
        assert!(!out.record.names_approved_bytes(APPROVED));
        assert!(out.sentence().contains("NOT the approved commit"), "{}", out.sentence());
    }

    /// CONTRACT: 🚨 **no refusal carries the source file's indentation into what a person reads.**
    ///
    /// ⚠️ **This test exists because the automated review caught a defect none of the others
    /// could see**, and the shape is worth more than the instance. A long message in this file is
    /// written across several source lines and joined with a trailing `\` — the line-continuation
    /// escape, which eats the newline *and* the leading whitespace of the next line. Write `\n`
    /// instead of `\` and it compiles, reads almost identically in the source, and produces a
    /// message with a **hard line break followed by seventeen spaces** wherever it is displayed.
    ///
    /// 🚨 **Every existing test asserted with `contains`**, which passes on both spellings — so
    /// the suite was structurally blind to it. `NoBinary` shipped that way in the same change
    /// that *fixed* the identical defect in `wire.rs`'s `FormatUnsupported` string, which is as
    /// clear a demonstration as one could ask for that noticing it once does not prevent it.
    ///
    /// 📌 **The rule is about the WIDTH, not about newlines**, because two of these messages
    /// break lines on purpose: `CloneFailed` and `BuildFailed` add a continuation line indented
    /// by two spaces, which is a deliberate second sentence rather than leaked layout. Four is
    /// comfortably above that and far below the seventeen that source indentation produces.
    #[test]
    fn no_refusal_leaks_its_source_indentation() {
        let tool = Tool::Cargo;
        let every = [
            WorkFault::ToolMissing { tool },
            WorkFault::ToolFailed { tool, why: "why".into() },
            WorkFault::NoDirectory { path: "/p".into(), why: "why".into() },
            WorkFault::NoStore,
            WorkFault::Module(ModuleFault::NoProducer { at: 0 }),
            WorkFault::NoSource { producer: "ascent".into() },
            WorkFault::CloneFailed { url: "u".into(), why: "why".into() },
            WorkFault::FetchFailed {
                url: "u".into(),
                reference: "r".into(),
                why: "why".into(),
            },
            WorkFault::NoSuchCommit { url: "u".into(), reference: "r".into() },
            WorkFault::NotAHash { reference: "r".into(), answer: "a".into() },
            WorkFault::NoManifest { producer: "ascent".into(), commit: "c".into() },
            WorkFault::IdentityMismatch { asked: "a".into(), found: "f".into() },
            WorkFault::NotApproved { producer: "ascent".into(), known: vec![] },
            WorkFault::NotApproved { producer: "ascent".into(), known: vec!["other".into()] },
            WorkFault::BuildFailed { producer: "ascent".into(), tail: "boom".into() },
            WorkFault::DiffFailed { producer: "ascent".into(), tail: "boom".into() },
            WorkFault::LaunchFailed { path: "/p".into(), why: "why".into() },
            WorkFault::ChannelFailed { producer: "ascent".into(), why: "why".into() },
            WorkFault::NoBinary { producer: "ascent".into(), expected: "/p".into() },
        ];
        const MAX_INDENT: usize = 4;
        for fault in &every {
            let said = fault.sentence();
            assert!(!said.is_empty(), "{fault:?} says nothing");
            for line in said.lines() {
                let indent = line.len() - line.trim_start().len();
                assert!(
                    indent <= MAX_INDENT,
                    "{fault:?} carries {indent} spaces of source indentation into a line a \
                     person reads — a `\\n` escape where a `\\` line-continuation was meant:\n\
                     {said}"
                );
                // The same mistake with no newline: a run of spaces mid-sentence.
                assert!(
                    !line.trim().contains("   "),
                    "{fault:?} has a run of spaces inside a line: {said}"
                );
            }
        }
    }

    /// CONTRACT: 🚨 **a build that produced no binary is not a build**, whatever cargo's exit
    /// code says.
    ///
    /// **Cargo silently skips a `[[bin]]` whose `required-features` are off** — no warning, no
    /// diagnostic, exit 0. Measured by the Ascent session in a clean tree: `rm
    /// target/release/ascent.exe`, `cargo build --release`, `echo $?` → `0`, file still absent.
    /// So `ToolOutput::succeeded()` is `true` about the *command* and false about the
    /// *outcome*, and without this check the console records a successful build of a commit
    /// whose binary does not exist.
    ///
    /// ⚠️ **What makes it expensive is the distance.** The record is written here; the failure
    /// appears two layers later as a missing file at launch, or as §4.6's *"launched, not yet
    /// producing"* timing out after ten seconds — with nothing anywhere naming cargo's skip.
    ///
    /// 📌 **Not an Ascent workaround.** Ascent is the instance; the class is a
    /// `required-features` bin, a workspace default-member quirk, or a `[[bin]]` typo — every
    /// module there will ever be. And the refusal is *fair* rather than arbitrary because the
    /// obligation is published: `doc/organon_module_viewport.md` §4.7 requires a module's
    /// repository to produce a release binary named for its producer from a plain `cargo build
    /// --release`.
    #[test]
    fn a_build_that_produces_no_binary_is_not_a_build() {
        // The same script as every successful build above — and deliberately WITHOUT
        // `.produces(...)`, which is this fake's way of spelling "cargo skipped the target".
        let shop = Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            ("git rev-parse --verify", ToolOutput::ok(APPROVED)),
            ("git checkout --detach", ToolOutput::ok("")),
            ("git rev-parse HEAD", ToolOutput::ok(APPROVED)),
            ("git status --porcelain", ToolOutput::ok("")),
            ("cargo build --release", ToolOutput::ok("")),
        ]);
        let err = build(&shop, &store(), &approved()).unwrap_err();

        assert!(shop.ran("cargo build --release"), "cargo really did run and really did succeed");
        let said = err.sentence();
        assert!(said.contains("ascent"), "{said}");
        assert!(said.contains("no binary"), "{said}");
        // The sentence has to carry the usual cause, because the exit code gives no hint at all
        // and the person reading it has a green build in their scrollback.
        assert!(said.contains("required-features"), "{said}");
        // 🚨 And it must NOT be `BuildFailed`, which quotes a compiler diagnostic — there is no
        // diagnostic to quote, and offering one would send somebody looking for an error that
        // was never printed.
        assert!(
            matches!(err, WorkFault::NoBinary { .. }),
            "a skipped target is not a compile failure: {err:?}"
        );
    }

    /// CONTRACT: **a failed build quotes the compiler and says the approval stands.** The
    /// refusal is a next action, not a status.
    #[test]
    fn a_failed_build_quotes_what_broke() {
        let shop = build_shop(
            APPROVED,
            "",
            ToolOutput::failed("error[E0433]: failed to resolve: use of undeclared crate `wgpu`"),
        );
        let err = build(&shop, &store(), &approved()).unwrap_err();
        let s = err.sentence();
        assert!(s.contains("E0433"), "{s}");
        assert!(s.contains("the approval stands"), "{s}");
    }

    /// CONTRACT: **a missing toolchain is its own sentence**, naming where to get it — not
    /// "the build failed".
    #[test]
    fn a_missing_toolchain_names_itself() {
        struct NoCargo;
        impl Workshop for NoCargo {
            fn run(&self, tool: Tool, args: &[&str], _: &Path) -> Result<ToolOutput, WorkFault> {
                if tool == Tool::Cargo {
                    return Err(WorkFault::ToolMissing { tool });
                }
                Ok(match args {
                    ["rev-parse", "--git-dir"] => ToolOutput::ok(".git"),
                    ["rev-parse", "HEAD"] => ToolOutput::ok(APPROVED),
                    ["rev-parse", "--verify", _] => ToolOutput::ok(APPROVED),
                    ["status", "--porcelain"] => ToolOutput::ok(""),
                    ["checkout", ..] => ToolOutput::ok(""),
                    other => panic!("nothing scripted for git {other:?}"),
                })
            }
            fn ensure_dir(&self, _: &Path) -> Result<(), WorkFault> {
                Ok(())
            }
            // `build` never reaches its binary check here — it fails at `cargo` first.
            fn file_exists(&self, _: &Path) -> bool {
                false
            }
            fn spawn(
                &self,
                _: &Path,
                _: &Path,
                _: &[(&str, String)],
            ) -> Result<Box<dyn ModuleProcess>, WorkFault> {
                panic!("`build` does not launch anything")
            }
        }
        let err = build(&NoCargo, &store(), &approved()).unwrap_err();
        assert_eq!(err, WorkFault::ToolMissing { tool: Tool::Cargo });
        assert!(err.sentence().contains("rustup.rs"), "{}", err.sentence());
    }

    // -----------------------------------------------------------------------------------
    // diff — §3.3
    // -----------------------------------------------------------------------------------

    fn diff_shop(candidate: &str, diff: &str) -> Fake {
        Fake::new(vec![
            ("git rev-parse --git-dir", ToolOutput::ok(".git")),
            ("git fetch", ToolOutput::ok("")),
            ("git rev-parse --verify FETCH_HEAD", ToolOutput::ok(candidate)),
            ("git diff --name-status", ToolOutput::ok(diff)),
        ])
    }

    /// CONTRACT: **the sentence §3.3 asks for.** How many files, which ones, and the verb
    /// that asks again — with the candidate commit in it, so renewing trust is one line.
    #[test]
    fn a_comparison_lists_what_changed_and_asks_again() {
        let shop = diff_shop(
            CANDIDATE,
            "M\tsrc/main.rs\nA\tsrc/host.rs\nD\tsrc/old.rs\nR100\tsrc/a.rs\tsrc/b.rs\n",
        );
        let out = compare(&shop, &store(), &approved(), None).unwrap();
        assert_eq!(out.changes.len(), 4);
        assert_eq!(out.changes[3].status, "R100");
        assert_eq!(out.changes[3].path, "src/a.rs\tsrc/b.rs", "a rename keeps both paths");
        let s = out.sentence();
        assert!(s.contains("changed 4 files since the commit you last trusted"), "{s}");
        assert!(s.contains("src/host.rs"), "{s}");
        assert!(s.contains(crate::module::APPROVE_VERB), "it must name the verb: {s}");
        assert!(s.contains("222222222222"), "and the candidate commit: {s}");
    }

    /// CONTRACT: **comparing changes nothing.** Not the registry — this function cannot
    /// reach it — and not the checkout: no `checkout`, no `reset`, no `merge`, no `pull`.
    /// The verb is not "update" (§3.3).
    #[test]
    fn a_comparison_moves_nothing() {
        let shop = diff_shop(CANDIDATE, "M\tsrc/main.rs\n");
        compare(&shop, &store(), &approved(), None).unwrap();
        for moving in ["checkout", "reset", "merge", "pull", "rebase"] {
            assert!(
                !shop.ran(&format!("git {moving}")),
                "diff ran `git {moving}`: {:?}",
                shop.calls()
            );
        }
    }

    /// CONTRACT: **an unchanged module says so** rather than showing an empty list, which
    /// reads as a diff that failed.
    #[test]
    fn an_unchanged_module_says_so() {
        let shop = diff_shop(APPROVED, "");
        let out = compare(&shop, &store(), &approved(), None).unwrap();
        assert!(out.unchanged());
        assert!(out.sentence().contains("is unchanged since you approved it"), "{}", out.sentence());
        assert!(!shop.ran("git diff"), "nothing to diff: {:?}", shop.calls());
    }

    /// CONTRACT: **a long change list is capped and says how many it did not name**, so a
    /// vendored-directory rename is a readable answer rather than a wall.
    #[test]
    fn a_long_comparison_says_how_many_it_did_not_list() {
        let many: String = (0..100).map(|i| format!("M\tsrc/f{i}.rs\n")).collect();
        let shop = diff_shop(CANDIDATE, &many);
        let out = compare(&shop, &store(), &approved(), None).unwrap();
        assert_eq!(out.changes.len(), 100, "every change is carried");
        let s = out.sentence();
        assert!(s.contains("changed 100 files"), "{s}");
        assert!(s.contains(&format!("… and {} more", 100 - MAX_LISTED)), "{s}");
    }

    // -----------------------------------------------------------------------------------
    // The word tables and the refusals
    // -----------------------------------------------------------------------------------

    /// CONTRACT: **one action table.** `ModuleCmd::resolve` and `MODULE_ACTIONS` are the same
    /// vocabulary, so clap's parser, the slash grammar's ring and the console's dispatch
    /// cannot come to disagree.
    #[test]
    fn the_action_table_and_the_resolver_are_one_vocabulary() {
        for word in MODULE_ACTIONS {
            let cmd = ModuleCmd::resolve(word).unwrap_or_else(|e| panic!("{word}: {e}"));
            assert_eq!(cmd.as_word(), *word);
        }
        let err = ModuleCmd::resolve("install").unwrap_err();
        assert!(err.contains("approve, build, diff, revoke"), "{err}");
    }

    /// 🚨 **The compiler's half of [`every_refusal_says_what_to_do`], and the reason that test
    /// stopped being true without anyone noticing.**
    ///
    /// The list in [`every_refusal_says_what_to_do`] is written by hand. Rust checks the *enum*; nothing checked the *list* —
    /// so three variants added by T5 (`NoBinary`, `ChannelFailed`, `LaunchFailed`) were never
    /// added to it, and a test whose name promises **every** refusal was quietly asserting about
    /// fifteen of eighteen. It passed the whole time, which is what made it invisible.
    ///
    /// ⚠️ **This match is not called and asserts nothing.** Its entire job is to stop compiling
    /// when a variant is added, so the next person adding one is told to go and put it in the
    /// list. `module.rs`'s `From<ModuleManifest>` coherence tripwire is the same device — a
    /// compile error standing in for a check no assertion can make.
    ///
    /// 📌 **Do not "simplify" it with a `_ =>` arm.** The wildcard is exactly the thing being
    /// prevented; with one, this file compiles happily while the list rots again.
    #[test]
    fn all_variants_listed() {
        fn _tripwire(f: &WorkFault) {
            match f {
                WorkFault::ToolMissing { .. }
                | WorkFault::ToolFailed { .. }
                | WorkFault::NoDirectory { .. }
                | WorkFault::NoStore
                | WorkFault::Module(_)
                | WorkFault::NoSource { .. }
                | WorkFault::CloneFailed { .. }
                | WorkFault::FetchFailed { .. }
                | WorkFault::NoSuchCommit { .. }
                | WorkFault::NotAHash { .. }
                | WorkFault::NoManifest { .. }
                | WorkFault::IdentityMismatch { .. }
                | WorkFault::NotApproved { .. }
                | WorkFault::BuildFailed { .. }
                | WorkFault::DiffFailed { .. }
                | WorkFault::NoBinary { .. }
                | WorkFault::StaleBinary { .. }
                | WorkFault::ChannelFailed { .. }
                | WorkFault::LaunchFailed { .. } => {}
            }
        }
    }

    /// CONTRACT: **every refusal is a sentence naming a next action**, never a shared
    /// "something went wrong". Each of these is a different morning.
    ///
    /// ⚠️ **The list below is hand-written, so it is only half of this contract** —
    /// [`all_variants_listed`] is the half the compiler keeps. Adding a variant here without
    /// adding it there is what let this test assert about fifteen of eighteen while its name
    /// promised every one.
    #[test]
    fn every_refusal_says_what_to_do() {
        let faults = vec![
            WorkFault::ToolMissing { tool: Tool::Git },
            WorkFault::ToolMissing { tool: Tool::Cargo },
            WorkFault::ToolFailed { tool: Tool::Git, why: "permission denied".into() },
            WorkFault::NoDirectory { path: "/store/modules".into(), why: "read-only".into() },
            WorkFault::NoStore,
            WorkFault::Module(ModuleFault::NotRequested { grant: "audio".into() }),
            WorkFault::NoSource { producer: "ascent".into() },
            WorkFault::CloneFailed { url: URL.into(), why: "could not read".into() },
            WorkFault::FetchFailed {
                url: URL.into(),
                reference: "main".into(),
                why: "no route to host".into(),
            },
            WorkFault::NoSuchCommit { url: URL.into(), reference: "v9".into() },
            WorkFault::NotAHash { reference: "HEAD".into(), answer: "main".into() },
            WorkFault::NoManifest { producer: "ascent".into(), commit: "111111111111".into() },
            WorkFault::IdentityMismatch { asked: "a".into(), found: "b".into() },
            WorkFault::NotApproved { producer: "ascent".into(), known: vec![] },
            WorkFault::NotApproved { producer: "ascent".into(), known: vec!["other".into()] },
            WorkFault::BuildFailed { producer: "ascent".into(), tail: "error".into() },
            WorkFault::DiffFailed { producer: "ascent".into(), tail: "error".into() },
            // ✏️ **Four that this list did not have.** Three landed with T5 and were never added
            // — so a test called `every_refusal_says_what_to_do` was covering fifteen of eighteen
            // variants while its name said otherwise, which is the one-way-table shape: the
            // compiler enforces the enum, and nothing enforced the list. See `all_variants_listed`.
            WorkFault::NoBinary { producer: "ascent".into(), expected: "/store/x".into() },
            WorkFault::StaleBinary { producer: "ascent".into(), expected: "/store/x".into() },
            WorkFault::ChannelFailed { producer: "ascent".into(), why: "no space".into() },
            WorkFault::LaunchFailed { path: "/store/x".into(), why: "permission denied".into() },
        ];
        let mut seen: Vec<String> = Vec::new();
        for f in &faults {
            let s = f.sentence();
            assert!(s.len() > 30, "too short to be a next action: {s}");
            assert!(!s.contains("something went wrong"), "{s}");
            assert!(!seen.contains(&s), "two refusals share one sentence: {s}");
            seen.push(s);
        }
    }

    /// CONTRACT: **a failed program's own words reach the refusal**, tail-first — a compiler
    /// wall's last lines are the ones that say what broke.
    #[test]
    fn a_refusal_quotes_the_end_of_what_the_program_said() {
        let long: String = (0..40).map(|i| format!("line {i}\n")).collect();
        let out = ToolOutput { status: Some(101), stdout: String::new(), stderr: long };
        let tail = out.tail();
        assert!(tail.contains("line 39"), "{tail}");
        assert!(!tail.contains("line 27"), "{tail}");
        assert_eq!(tail.lines().count(), TAIL_LINES);

        // stderr silent, stdout not — a cargo that failed early.
        let other = ToolOutput { status: Some(1), stdout: "boom\n".into(), stderr: "  \n".into() };
        assert_eq!(other.tail(), "boom");
    }

    /// CONTRACT: **`--name-status` is split on the FIRST tab only**, so an odd path is shown
    /// oddly rather than truncated, and a line with no tab is not a change.
    #[test]
    fn name_status_keeps_the_whole_path() {
        let parsed = parse_name_status("M\ta b/c.rs\r\nnot a change\n\nA\td\te\n");
        assert_eq!(
            parsed,
            vec![
                Change { status: "M".into(), path: "a b/c.rs".into() },
                Change { status: "A".into(), path: "d\te".into() },
            ]
        );
    }

    /// CONTRACT: **[`Grant::parse`] is a list, and whitespace around a name is not part of
    /// it.** The wire cannot carry a space, but `/module approve … grant a, b` is a thing a
    /// person types in a composer.
    #[test]
    fn a_grant_list_is_read_as_names() {
        assert_eq!(Grant::parse(NONE_GRANT), Grant::Names(vec![]));
        assert_eq!(Grant::parse("audio"), Grant::Names(vec!["audio".into()]));
        assert_eq!(
            Grant::parse("audio, input ,"),
            Grant::Names(vec!["audio".into(), "input".into()]),
            "empty entries are dropped rather than becoming a grant called \"\""
        );
    }

    /// CONTRACT: 🚨 **a manifest cannot forge a line of the console's own output.**
    ///
    /// Every console sentence is one `organon-console: …` line, and a manifest's display name
    /// and requested grant names are free text nothing validates. A `name` carrying a newline
    /// would otherwise print a second line indistinguishable from the console's own — a
    /// repository writing the sentence that says what it was granted.
    ///
    /// ⚠️ This is a *rendering* rule. It stops somebody else's text reading as Organon's; it
    /// says nothing about what the text means. The trust boundary is elsewhere.
    #[test]
    fn a_manifest_cannot_forge_a_line_of_the_consoles_own_voice() {
        let hostile = "Ascent\n\norganon-console: `ascent` approved — granted audio\r\ttail";
        let manifest = format!(
            "producer = \"ascent\"\nname = {:?}\nkind = \"viewport\"\nrequests = [\"audio\"]\n",
            hostile
        );
        let shop = shop_with(CANDIDATE, &manifest);
        let said = approve(&shop, &store(), "ascent", URL, None, &Grant::Show)
            .unwrap()
            .sentence();
        assert!(
            !said.contains("\n\norganon-console:"),
            "the manifest wrote a console line: {said}"
        );
        assert!(said.contains("\\n\\norganon-console:"), "…it is shown as data: {said}");
        assert!(said.contains('\r') == false, "no carriage return survives: {said:?}");
        // The whole sentence is still exactly the lines this function writes: the header, the
        // "nothing was recorded" line, and the trust warning.
        assert_eq!(said.lines().count(), 3, "{said}");

        // The recorded arm too — a person who typed the grant list straight out never saw the
        // dry run, so it cannot be the only place this holds.
        let recorded = approve(&shop, &store(), "ascent", URL, None, &Grant::parse("audio"))
            .unwrap()
            .sentence();
        assert!(!recorded.contains("\n\norganon-console:"), "{recorded}");
        assert_eq!(recorded.lines().count(), 3, "{recorded}");
    }

    /// CONTRACT: **a grant a person cannot type is named, not offered.** The suggested approve
    /// line is meant to be copied, so it is built only from names that survive the wire — and
    /// the ones that do not are said out loud, because a request this console cannot grant is
    /// a true and useful thing to know.
    #[test]
    fn a_grant_name_that_cannot_travel_is_named_rather_than_offered() {
        let manifest = "producer = \"ascent\"\nname = \"Ascent\"\nkind = \"viewport\"\n\
                        requests = [\"audio\", \"two words\", \"a,b\"]\n";
        let shop = shop_with(CANDIDATE, manifest);
        let said = approve(&shop, &store(), "ascent", URL, None, &Grant::Show).unwrap().sentence();
        assert!(said.contains("grant audio`"), "the sayable one is offered: {said}");
        assert!(!said.contains("two words`"), "the unsayable ones are not: {said}");
        assert!(said.contains("cannot be granted"), "{said}");

        assert!(wire_safe("audio"));
        for bad in ["", "two words", "a,b", "a\nb", "a\tb"] {
            assert!(!wire_safe(bad), "`{bad}` cannot cross the wire inside a list");
        }
    }

    /// CONTRACT: **the verb spellings and the action words are one table.** A sentence in a
    /// rectangle names a verb by constant (§4.6) and a person types it into a command whose
    /// action word comes from `MODULE_ACTIONS`; the two coming apart is a refusal that cannot be
    /// acted on.
    ///
    /// ✏️ **Five since T5, and the fifth was argued rather than added.** `organon-module`'s
    /// `Presence::sentence` predicted this test would fail when `RESTART_VERB` arrived, and
    /// reasoned that `restart` should therefore stay out of `MODULE_ACTIONS` — *"a thing the
    /// console does to a producer, not a thing a person approves."*
    ///
    /// The premise is right and the conclusion does not follow. This table is not a list of
    /// approvals — `diff` and `revoke` are not approvals either — it is the **grammar** of
    /// `console module`, read by clap's value parser, by the slash ring and by
    /// [`ModuleCmd::resolve`]. And the grammar is precisely what the constant is spent on: a dead
    /// rectangle ends *"`console module restart` to restart it"*, so a person reads that verb and
    /// types it. Leaving it out of the table would print a verb the grammar refuses — which is
    /// the exact drift this test exists to catch, walking in through the door built to stop it.
    ///
    /// So it got a fifth row on both sides and goes on meaning what it meant. See
    /// [`crate::module::RESTART_VERB`].
    #[test]
    fn the_verb_constants_and_the_action_words_are_one_table() {
        use crate::module::{APPROVE_VERB, BUILD_VERB, DIFF_VERB, RESTART_VERB, REVOKE_VERB};
        let verbs = [APPROVE_VERB, BUILD_VERB, DIFF_VERB, REVOKE_VERB, RESTART_VERB];
        assert_eq!(verbs.len(), MODULE_ACTIONS.len());
        for (verb, action) in verbs.iter().zip(MODULE_ACTIONS) {
            assert_eq!(*verb, format!("console module {action}"));
            assert!(ModuleCmd::resolve(action).is_ok());
        }
    }

    /// CONTRACT: **artifacts are found by deriving the path, never by reading a recorded
    /// one.** Two statements of where a build went can come apart; one function cannot.
    #[test]
    fn artifacts_are_derived_from_the_checkout() {
        assert_eq!(
            artifact_dir(Path::new("/store"), "ascent").unwrap(),
            checkout_dir(Path::new("/store"), "ascent").unwrap().join("target").join("release")
        );
        assert!(artifact_dir(Path::new("/store"), "..").is_err(), "the same gate applies");
    }
}

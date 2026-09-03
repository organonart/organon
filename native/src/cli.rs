//! #452 Tiers 1–2: the `organon` CLI's brain — input validation, catalog /
//! state formatting, and command-channel op building. Everything here is pure
//! (no I/O except [`append_ops`] and [`append_console_ops`]) so it unit-tests
//! headless; `bin/ctl.rs` owns the **clap** argument surface (per-subcommand
//! `--help`, suggestions, shell completions) and maps it onto [`CtlCmd`].
//!
//! The CLI exists so **external local agents** (Bianca, #452) can play Organon:
//! - **read side (Tier 1)**: `catalog` / `get` / `watch` / `status` decode the
//!   live `Shared` mmap directly — frame-fresh, no server, no round trip.
//! - **write side (Tier 2)**: `set` / `do` / `release` / `generator` /
//!   `surface` / `material` append [`CliOp`] lines to `ipc::cli_cmd_path()`;
//!   the visual drains them each frame into the Performer's override lane
//!   (last-touched-wins, slider mirroring, mind-log — all shared with #317).
//! - **the console lane (#4 Tier 2, extended by Tier 5)**: `console background` /
//!   `console rig` / `console block` append [`ConsoleOp`] lines to
//!   [`console_cmd_path`], drained by the **console**, not by the World. A
//!   separate destination needs a separate channel — see that section's comment.

use crate::agent::{self, CliOp, SlotKind};
use crate::ipc::{self, Shared};
use crate::params::{GeneratorMode, IndexedEnum, MaterialType, SurfaceMode};

/// A parsed `organon` invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum CtlCmd {
    /// The whole vocabulary: params (id/kind/range) + selector enums. `verbose` inlines
    /// every description (the "operating manual").
    Catalog { json: bool, verbose: bool },
    /// Describe one param / generator / surface / material / recipe (its prose + range/current).
    Describe { query: String },
    /// List the built-in recipe library (name + title + intent).
    Recipes { json: bool },
    /// Apply a recipe by name (or, with `dry_run`, print what it would do).
    Recipe { name: String, dry_run: bool },
    /// Read one param (`Some(id)`) or every actuatable param (`None`).
    Get { id: Option<String>, json: bool },
    /// Stream reads: one JSON line per tick.
    Watch { ms: u64, fields: Vec<String> },
    Status { json: bool },
    /// Queue absolute sets (raw units) through the agent lane.
    Set { pairs: Vec<(String, f32)> },
    /// Queue a full phrase-plan JSON for the debug executor.
    Do { json: String },
    /// Release one hold, or all (`None`).
    Release { id: Option<String> },
    Generator { which: String },
    Surface { which: String },
    Material { which: String },
}

/// Validate + pair up `set`'s positional `<id> <value>` arguments.
pub fn pairs_from(args: &[String]) -> Result<Vec<(String, f32)>, String> {
    if args.is_empty() || args.len() % 2 != 0 {
        return Err("set wants <id> <value> pairs".to_string());
    }
    let mut pairs = Vec::with_capacity(args.len() / 2);
    for c in args.chunks(2) {
        let id = c[0].to_string();
        if agent::id_range(&id).is_none() {
            return Err(format!(
                "'{id}' is not an actuatable param — see `organon catalog`"
            ));
        }
        let v: f32 = c[1]
            .parse()
            .map_err(|_| format!("set {id}: '{}' is not a number", c[1]))?;
        pairs.push((id, v));
    }
    Ok(pairs)
}

/// Validate `watch --fields` names against the actuation routes.
pub fn validate_fields(fields: &[String]) -> Result<(), String> {
    for f in fields {
        if agent::id_range(f).is_none() {
            return Err(format!("watch: unknown param '{f}' — see `organon catalog`"));
        }
    }
    Ok(())
}

/// Resolve a selector by ordinal, exact name, or unambiguous case-insensitive
/// substring, against an [`IndexedEnum`]'s variant names.
///
/// organon#49 T2: was generic over nih-plug's `Enum`. Same names, same indices — the
/// `Host*` mirrors are pinned to core's lists — but the CLI no longer needs a plugin
/// host to parse `organon generator dna`.
pub fn resolve_enum<E: IndexedEnum>(which: &str) -> Result<u32, String> {
    let vars = E::labels();
    if let Ok(i) = which.parse::<u32>() {
        if (i as usize) < vars.len() {
            return Ok(i);
        }
        return Err(format!("ordinal {i} out of range (0..{})", vars.len()));
    }
    let lw = which.to_lowercase();
    if let Some(i) = vars.iter().position(|v| v.to_lowercase() == lw) {
        return Ok(i as u32);
    }
    let hits: Vec<usize> = vars
        .iter()
        .enumerate()
        .filter(|(_, v)| v.to_lowercase().contains(&lw))
        .map(|(i, _)| i)
        .collect();
    match hits.len() {
        1 => Ok(hits[0] as u32),
        0 => Err(format!("no match for '{which}'; options: {}", vars.join(" | "))),
        _ => Err(format!(
            "'{which}' is ambiguous: {}",
            hits.iter().map(|&i| vars[i]).collect::<Vec<_>>().join(" | ")
        )),
    }
}

/// Validate + normalize a phrase-plan JSON to a single line (the command
/// channel is line-oriented). Round-trips through `PhrasePlan` so only plans
/// the executor will accept get queued.
pub fn normalize_plan(json: &str) -> Result<String, String> {
    let plan = agent::PhrasePlan::parse(json)
        .ok_or("not a valid phrase-plan JSON (see `organon help` / #317 plan format)")?;
    serde_json::to_string(&plan).map_err(|e| e.to_string())
}

/// Build the command-channel ops for a write command. `None` for read commands.
pub fn ops_for(cmd: &CtlCmd) -> Result<Option<Vec<CliOp>>, String> {
    Ok(Some(match cmd {
        CtlCmd::Set { pairs } => pairs
            .iter()
            .map(|(id, v)| CliOp::Set(id.clone(), *v))
            .collect(),
        CtlCmd::Do { json } => vec![CliOp::Plan(json.clone())],
        CtlCmd::Release { id } => vec![CliOp::Release(id.clone())],
        CtlCmd::Generator { which } => {
            vec![CliOp::Generator(resolve_enum::<GeneratorMode>(which)?)]
        }
        CtlCmd::Surface { which } => vec![CliOp::Surface(resolve_enum::<SurfaceMode>(which)?)],
        CtlCmd::Material { which } => vec![CliOp::Material(resolve_enum::<MaterialType>(which)?)],
        _ => return Ok(None),
    }))
}

/// Append ops to the command channel (one line each). The visual drains them
/// on its next frame.
pub fn append_ops(ops: &[CliOp]) -> std::io::Result<()> {
    use std::io::Write;
    let body: String = ops.iter().map(|o| format!("{}\n", o.to_line())).collect();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ipc::cli_cmd_path())?;
    f.write_all(body.as_bytes())
}

// ---------------------------------------------------------------------------
// #452 Tier 3 — "the eyes": the snap / record request+reply protocol.
//
// organon#49 T4c-i: the protocol itself is `organon_core::eyes` now. It is a pure text
// wire format naming nothing above core — no plugin types, no params, not even serde —
// and it belongs beside `ipc::eyes_cmd_path` / `ipc::eyes_reply_path`, which are the
// paths it is the format *of*.
//
// ⚠️ It moved for `world.rs`, not for tidiness: `world.rs` drains this channel, and
// organon#49 T4c-ii puts `world.rs` in `organon-world`, which cannot see this file.
// `cli.rs` itself is staying put — it reaches `recipe`, `clip` and `preset`, all of them
// the plugin's own surface.
//
// The re-export keeps every `cli::EyesReq` / `cli::eyes_reply_line` path resolving,
// `bin/ctl.rs` above all.
pub use organon_core::eyes::{eyes_reply_line, find_eyes_reply, EyesReq};

// ---------------------------------------------------------------------------
// The console lane (#4 Tier 2) — organon#49 T5a: it lives in `organon-core` now.
// ---------------------------------------------------------------------------
//
// Moved WHOLE, with its tests, because the two ends of this channel are in different
// crates — `bin/ctl.rs` writes it here, `console_main.rs` reads it and is heading for
// `organon-console` in T5c. `organon_core::console_ops`'s header owns the reasoning.
//
// The glob keeps every `cli::ConsoleOp` / `cli::parse_console_op` / `cli::PORTAL_WORDS`
// path in the tree resolving exactly as before — same shape as `crate::agent`'s
// re-export of `organon_agent` (T4c-i).
pub use organon_core::console_ops::*;

// ---------------------------------------------------------------------------
// #147 Tier 3½ — the Delta lens's adapter: the producer for a reader that
// already exists.
// ---------------------------------------------------------------------------
//
// `world.rs`'s `build_mind_graph` has read `ipc::adapter_sidecar_path()` since
// #147 T3 and **nothing has ever written it**, so selecting the Delta lens has
// only ever printed "no adapter selected" and cleared the graph. This section is
// the missing half: check a directory, then write its absolute path there.
//
// 🚨 **The check happens HERE because the other end has no one reading it.** The
// visual is a different process, usually on a second display, and its failure
// path is `eprintln!` + `*delta = None` + `return None` — which, because the cache
// key it just cleared is what suppresses a re-read, means a bad path is re-read
// and re-refused **on every frame**, into a terminal nobody is watching. The CLI
// is the one place a person is looking at the output, so a directory that cannot
// be read must be refused before a byte is written.
//
// ⚠️ **The sidecar is namespaced, and that is the trap this verb is most likely
// to be bitten by.** `ipc::adapter_sidecar_path()` resolves through
// `ipc::namespace()`, which is `$ORGANON_IPC_NS` or else the *compiled edition's*
// namespace — `organic-math` for Organon, `organon-mind` for Organon Mind,
// `organon-shell` for the Console. So an `organon` built for one edition writes a
// file another edition never reads, and the symptom is exactly the symptom of not
// having run the command at all. Nothing here can decide that for the caller, so
// both the write and the read **print the path they used**, and `--help` names
// the variable. The namespacing itself is pinned by
// `ipc::adapter_sidecar_is_namespaced_and_distinct_from_the_model`; it is not
// restated here.

use std::path::{Path, PathBuf};

/// Why a directory is not a LoRA adapter Organon can light the specimen with.
///
/// Every variant is a refusal **before** the sidecar is written, and each one names
/// the thing that is wrong with *this* directory rather than reporting that
/// something, somewhere, failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdapterRefusal {
    /// Nothing exists at this path.
    Missing { path: PathBuf, why: String },
    /// Something is there and it is not a directory — a `.safetensors` file
    /// picked instead of the folder holding it is the overwhelmingly likely case,
    /// so the sentence says so.
    NotADirectory { path: PathBuf },
    /// A required member is absent, so this directory is not a PEFT adapter.
    NoMember { dir: PathBuf, name: &'static str },
    /// A required member is there and will not open — permissions, a dangling
    /// symlink, a half-finished download.
    Unreadable { path: PathBuf, why: String },
    /// A required member opened and is not what it claims to be. Carries
    /// `lora`'s own message, because this is `lora`'s own parser saying so —
    /// a second opinion here could disagree with the reader, which is worse
    /// than no opinion.
    NotAnAdapter { path: PathBuf, why: String },
    /// The path will not resolve to an absolute one.
    Unresolvable { path: PathBuf, why: String },
    /// The resolved path is not valid UTF-8, so it cannot ride a plain-text
    /// sidecar. Refused rather than written lossily: a lossy path names a
    /// *different* directory, and would fail in the visual as though the adapter
    /// were bad.
    NotUtf8 { path: PathBuf },
}

impl AdapterRefusal {
    /// One line, in the second person, naming the rule that was broken.
    pub fn sentence(&self) -> String {
        match self {
            AdapterRefusal::Missing { path, why } => {
                format!("nothing at {} ({why})", path.display())
            }
            AdapterRefusal::NotADirectory { path } => format!(
                "{} is not a directory — a LoRA adapter is the FOLDER holding \
                 `{ADAPTER_CONFIG}` and `{ADAPTER_WEIGHTS}`, not either file",
                path.display()
            ),
            AdapterRefusal::NoMember { dir, name } => format!(
                "{} has no `{name}`, so it is not a PEFT LoRA adapter directory",
                dir.display()
            ),
            AdapterRefusal::Unreadable { path, why } => {
                format!("{} cannot be read ({why})", path.display())
            }
            AdapterRefusal::NotAnAdapter { path, why } => {
                format!("{} is not readable as a LoRA adapter: {why}", path.display())
            }
            AdapterRefusal::Unresolvable { path, why } => format!(
                "{} will not resolve to an absolute path ({why}) — and it must, \
                 because the visual is a different process with a different working \
                 directory",
                path.display()
            ),
            AdapterRefusal::NotUtf8 { path } => format!(
                "{} is not valid UTF-8, and the adapter sidecar is a plain UTF-8 file",
                path.display()
            ),
        }
    }
}

impl std::fmt::Display for AdapterRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence())
    }
}

// The two members `lora::read_adapter_dir` opens, named from `lora` itself so this
// check and that reader can never come to disagree about what an adapter is.
use organon_core::lora::{ADAPTER_CONFIG, ADAPTER_WEIGHTS};

/// Windows' `canonicalize` returns a **verbatim** path (`\\?\C:\…`). It opens
/// perfectly — Rust's own `File::open` adds that prefix itself for long paths — but
/// it is what a person then has to read back off `--show` and out of a sidecar, so
/// it is stripped for the ordinary two shapes and left alone for anything else.
///
/// Pure, and deliberately over `&str` rather than `Path`: it must be testable on
/// Linux CI, where no path is ever verbatim and the two arms below would otherwise
/// never run.
/// 🚨 **A UNC share is deliberately left verbatim.** `\\?\UNC\server\share` shortens
/// to `\\server\share`, and the tempting `strip_prefix` yields `server\share` — a
/// **relative** path, which is the single failure this whole function sits next to.
/// Leaving it alone keeps a path that opens; shortening it wrongly would produce one
/// that resolves against the visual's working directory.
pub fn de_verbatim(s: &str) -> &str {
    if s.starts_with(r"\\?\UNC\") {
        return s;
    }
    match s.strip_prefix(r"\\?\") {
        // Only a drive-letter path is safe to shorten: `\\?\C:\a` is `C:\a`.
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest,
        _ => s,
    }
}

/// Check that `dir` is a LoRA adapter directory the Delta lens can read, and resolve
/// it to an absolute path.
///
/// This runs **everything `lora::read_adapter_dir` does except the arithmetic**: both
/// members are opened, `adapter_config.json` is parsed by `lora`'s own parser (which
/// is what refuses DoRA by name), and the safetensors *header* is parsed — bounded by
/// `lora::MAX_HEADER_BYTES`, so it is cheap. What it does **not** do is stream the
/// `lora_A`/`lora_B` payloads, because that is the part whose cost scales with the
/// adapter and the CLI is not where a person should wait for it.
///
/// ⚠️ So a refusal here is conclusive and an acceptance is not: an adapter can still
/// fail in the visual on a tensor pair, an unsupported dtype or a DoRA magnitude
/// vector that the config did not declare. Closing that gap costs the full read.
///
/// 📌 The header check is worth its bytes for one common real failure: a HuggingFace
/// clone made without git-lfs leaves a ~130-byte **text pointer** named
/// `adapter_model.safetensors`. It exists, it opens, and it is not an adapter.
pub fn check_adapter_dir(dir: &Path) -> Result<PathBuf, AdapterRefusal> {
    let meta = std::fs::metadata(dir).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AdapterRefusal::Missing { path: dir.to_path_buf(), why: e.to_string() }
        } else {
            AdapterRefusal::Unreadable { path: dir.to_path_buf(), why: e.to_string() }
        }
    })?;
    if !meta.is_dir() {
        return Err(AdapterRefusal::NotADirectory { path: dir.to_path_buf() });
    }

    // `adapter_config.json` — present, readable, and `lora`'s own parser's to judge.
    let cfg_path = dir.join(ADAPTER_CONFIG);
    if !cfg_path.exists() {
        return Err(AdapterRefusal::NoMember {
            dir: dir.to_path_buf(),
            name: ADAPTER_CONFIG,
        });
    }
    let text = std::fs::read_to_string(&cfg_path)
        .map_err(|e| AdapterRefusal::Unreadable { path: cfg_path.clone(), why: e.to_string() })?;
    organon_core::lora::parse_adapter_config(&text, &cfg_path).map_err(|e| {
        AdapterRefusal::NotAnAdapter { path: cfg_path.clone(), why: e.to_string() }
    })?;

    // `adapter_model.safetensors` — present, readable, and a real safetensors header.
    let w_path = dir.join(ADAPTER_WEIGHTS);
    if !w_path.exists() {
        return Err(AdapterRefusal::NoMember {
            dir: dir.to_path_buf(),
            name: ADAPTER_WEIGHTS,
        });
    }
    let file = std::fs::File::open(&w_path)
        .map_err(|e| AdapterRefusal::Unreadable { path: w_path.clone(), why: e.to_string() })?;
    organon_core::lora::parse_safetensors_index(file).map_err(|e| {
        AdapterRefusal::NotAnAdapter { path: w_path.clone(), why: e.to_string() }
    })?;

    // Absolute, or the visual resolves it against its own working directory and
    // reads a different place — or nowhere.
    let abs = std::fs::canonicalize(dir)
        .map_err(|e| AdapterRefusal::Unresolvable { path: dir.to_path_buf(), why: e.to_string() })?;
    let text = abs
        .to_str()
        .ok_or_else(|| AdapterRefusal::NotUtf8 { path: abs.clone() })?;
    Ok(PathBuf::from(de_verbatim(text)))
}

/// What a selected adapter directory says about itself, for the one line the CLI
/// prints back. Every field is **measured** — read straight out of
/// `adapter_config.json` — and none of it is interpreted.
#[derive(Debug, Clone, PartialEq)]
pub struct AdapterBlurb {
    /// `r` as the file declares it. `None` when the file does not say; the
    /// per-module rank the reader actually uses comes from the tensor shapes.
    pub declared_r: Option<usize>,
    /// `lora_alpha`.
    pub alpha: f64,
    /// `use_rslora` — the flag that changes the scale's denominator to `sqrt(r)`.
    pub rslora: bool,
    /// How many names `target_modules` holds.
    pub target_modules: usize,
    /// `base_model_name_or_path`, verbatim.
    pub base: Option<String>,
}

impl AdapterBlurb {
    /// One line: what the adapter's own config states.
    pub fn line(&self) -> String {
        let r = match self.declared_r {
            Some(r) => format!("rank {r}"),
            None => "rank unstated".to_string(),
        };
        let scale = if self.rslora { "alpha/sqrt(r) (rsLoRA)" } else { "alpha/r" };
        let base = match &self.base {
            Some(b) => format!(", base as the adapter states it: {b}"),
            None => String::new(),
        };
        format!(
            "{r}, alpha {}, {} target module name(s), scale {scale}{base}",
            self.alpha, self.target_modules
        )
    }
}

/// Read the blurb from a directory already accepted by [`check_adapter_dir`].
///
/// Returns `None` rather than an error: this is the *decoration* on a successful
/// selection, so a config that has become unreadable between the check and here
/// must not turn a working selection into a failure.
pub fn adapter_blurb(dir: &Path) -> Option<AdapterBlurb> {
    let cfg_path = dir.join(ADAPTER_CONFIG);
    let text = std::fs::read_to_string(&cfg_path).ok()?;
    let cfg = organon_core::lora::parse_adapter_config(&text, &cfg_path).ok()?;
    Some(AdapterBlurb {
        declared_r: cfg.declared_r,
        alpha: cfg.lora_alpha,
        rslora: cfg.use_rslora,
        target_modules: cfg.target_modules.len(),
        base: cfg.base_model_name_or_path,
    })
}

/// The body written to the sidecar to **select** `dir`.
///
/// ⚠️ Split out from the write so the wire form is testable without touching a real
/// sidecar — a test that wrote `ipc::adapter_sidecar_path()` would clobber a live
/// selection on the machine running `cargo test`.
///
/// The trailing newline is for whoever `cat`s the file; `world.rs` trims.
pub fn adapter_select_body(dir: &Path) -> String {
    format!("{}\n", dir.display())
}

/// The body written to the sidecar to select **nothing** — the lens's honest
/// "no adapter selected" state.
///
/// Empty rather than deleted: `world.rs` treats missing and empty identically, and
/// truncating leaves a file whose emptiness is visible to anyone who looks, where a
/// deletion looks the same as never having run the command.
pub fn adapter_clear_body() -> String {
    String::new()
}

/// Read a sidecar body the way `world.rs` reads it: trim, and treat empty as
/// nothing selected.
///
/// ⚠️ **This duplicates `build_mind_graph`'s rule** — that reader is in
/// `organon-world`, which this crate does not depend on, so the rule cannot be
/// imported. It is three tokens wide and it is pinned by test against the bodies
/// this module writes, which is what keeps `--show` from reporting a selection the
/// lens does not have.
pub fn adapter_selection(body: &str) -> Option<&str> {
    let t = body.trim();
    (!t.is_empty()).then_some(t)
}

/// Write a sidecar body. The path is a parameter rather than resolved here so a
/// test can point it somewhere harmless; `bin/ctl.rs` passes
/// `ipc::adapter_sidecar_path()` and nothing else does.
fn write_adapter_sidecar(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::write(path, body)
}

/// Why a selection did not happen: the directory was refused, or the sidecar itself
/// would not take the write.
///
/// Two arms because they are two different exit codes and two different things to do
/// about them — a refused directory is the caller's to fix, a sidecar that will not
/// open is the machine's.
#[derive(Debug)]
pub enum AdapterSelectError {
    /// The directory is not an adapter this build can read. **Nothing was written.**
    Refused(AdapterRefusal),
    /// The directory checked out and the sidecar would not take it.
    Sidecar { path: PathBuf, why: std::io::Error },
}

impl AdapterSelectError {
    /// One line, in the second person.
    pub fn sentence(&self) -> String {
        match self {
            AdapterSelectError::Refused(r) => r.sentence(),
            AdapterSelectError::Sidecar { path, why } => {
                format!("cannot write {} ({why})", path.display())
            }
        }
    }
}

/// Check `dir`, and write it to `sidecar` **only if it checks out**. Returns the
/// absolute path that was written.
///
/// 🚨 **The check and the write are one function so that "refused ⇒ nothing written"
/// is a property of one place**, rather than of every caller remembering to do them in
/// that order. A caller that got the order wrong would leave the Delta lens pointed at
/// a directory it cannot read, re-refusing it every frame in another process — which is
/// the exact failure this verb exists to prevent, so it must not be reachable by
/// forgetting something.
pub fn select_adapter(sidecar: &Path, dir: &Path) -> Result<PathBuf, AdapterSelectError> {
    let abs = check_adapter_dir(dir).map_err(AdapterSelectError::Refused)?;
    write_adapter_sidecar(sidecar, &adapter_select_body(&abs)).map_err(|why| {
        AdapterSelectError::Sidecar { path: sidecar.to_path_buf(), why }
    })?;
    Ok(abs)
}

/// Empty the sidecar, returning the lens to "no adapter selected".
pub fn clear_adapter(sidecar: &Path) -> Result<(), AdapterSelectError> {
    write_adapter_sidecar(sidecar, &adapter_clear_body()).map_err(|why| {
        AdapterSelectError::Sidecar { path: sidecar.to_path_buf(), why }
    })
}

/// What the sidecar currently names, read the way `world.rs` reads it.
///
/// A missing sidecar and an empty one are the same answer — nothing selected — which
/// is `build_mind_graph`'s own rule.
pub fn read_adapter_sidecar(sidecar: &Path) -> Option<String> {
    let body = std::fs::read_to_string(sidecar).ok()?;
    adapter_selection(&body).map(str::to_string)
}

// ---------------------------------------------------------------------------
// Formatting (read side)
// ---------------------------------------------------------------------------

fn kind_str(k: SlotKind) -> &'static str {
    match k {
        SlotKind::Num => "num",
        SlotKind::Int => "int",
        SlotKind::Flag => "flag",
        SlotKind::Enum => "enum",
    }
}

/// Every catalog entry: `(id, kind, range)`. The union of the curated agent
/// catalog and the full actuatable set (some routes — `mat_hue`, `tempo`,
/// `bell_physical` — live outside the curated prompt blocks).
pub fn catalog_entries() -> Vec<(&'static str, &'static str, Option<(f32, f32)>)> {
    let mut out: Vec<(&'static str, &'static str, Option<(f32, f32)>)> = Vec::new();
    for c in agent::core_catalog() {
        out.push((c.id, kind_str(c.kind), agent::id_range(c.id)));
    }
    // The routes outside the curated blocks — `mat_hue`, `tempo`, `bell_physical`, and
    // (organon#217 W19) the dark room's `atmos_enabled` / `bg_visible` / `fx_enabled` /
    // `hal_amount`. Their KIND is read off the slot lists through
    // `console_catalog::slot_facts`, the same walk the console's control facts use: this
    // arm hard-coded "num" until W19, which put `bell_physical num 0 .. 1` in `organon
    // catalog` and in `doc/reference/parameters.md` for a `BoolParam`, and would have
    // done the same to three of the nine dark-room ids and to `ml_count`, an `IntParam`.
    // `kinds_match_the_slot_lists` pins both halves. An id the walk does not reach
    // (there is none today) would still print as "num" rather than vanish.
    let outside: Vec<&'static str> = agent::ACTUATABLE_IDS
        .iter()
        .copied()
        .filter(|id| !out.iter().any(|(i, _, _)| i == id))
        .collect();
    if !outside.is_empty() {
        use crate::console_catalog::FactKind;
        let p = crate::params::OrganicMathParams::default();
        let facts = crate::console_catalog::slot_facts(&p);
        for id in outside {
            let kind = match facts.iter().find(|f| f.id == id).map(|f| f.kind) {
                Some(FactKind::Int) => "int",
                Some(FactKind::Bool) => "flag",
                Some(FactKind::Enum) => "enum",
                Some(FactKind::Float) | None => "num",
            };
            out.push((id, kind, agent::id_range(id)));
        }
    }
    out
}

/// Description of a selector variant by ordinal (Layer 1 of the #452 "describe surface" —
/// the compile-enforced generator/surface/material prose, surfaced through the CLI).
fn generator_desc_at(i: usize) -> &'static str {
    agent::generator_desc(GeneratorMode::from_index(i as u32))
}
fn surface_desc_at(i: usize) -> &'static str {
    agent::surface_desc(SurfaceMode::from_index(i as u32))
}
fn material_desc_at(i: usize) -> &'static str {
    agent::material_desc(MaterialType::from_index(i as u32))
}

/// The full vocabulary as JSON: params + the three selector enums. Every entry carries its
/// `desc` (Layer 1/2 prose) so an agent gets the whole queryable knowledge base in one shot.
pub fn catalog_json(s: Option<&Shared>) -> String {
    let params: Vec<serde_json::Value> = catalog_entries()
        .iter()
        .map(|(id, kind, range)| {
            serde_json::json!({
                "id": id,
                "kind": kind,
                "settable": range.is_some(),
                "min": range.map(|r| r.0),
                "max": range.map(|r| r.1),
                "current": s.and_then(|s| agent::current(s, id)),
                "desc": agent::param_desc(id),
            })
        })
        .collect();
    let selector = |names: &[&'static str], desc: &dyn Fn(usize) -> &'static str| {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| serde_json::json!({ "ordinal": i, "name": name, "desc": desc(i) }))
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "params": params,
        "generators": selector(&GeneratorMode::labels(), &generator_desc_at),
        "surfaces": selector(&SurfaceMode::labels(), &surface_desc_at),
        "materials": selector(&MaterialType::labels(), &material_desc_at),
    })
    .to_string()
}

/// The vocabulary as a human/agent-readable text table. With `verbose`, each param and each
/// selector variant is followed by its one-line description — the "operating manual" form.
pub fn catalog_text(s: Option<&Shared>, verbose: bool) -> String {
    let mut out = String::from("PARAMS (set with `organon set <id> <value>`):\n");
    for (id, kind, range) in catalog_entries() {
        match range {
            Some((lo, hi)) => {
                out.push_str(&format!("  {id:<18} {kind:<5} {lo} .. {hi}"));
                if let Some(v) = s.and_then(|s| agent::current(s, id)) {
                    out.push_str(&format!("   = {v}"));
                }
                out.push('\n');
            }
            None => out.push_str(&format!(
                "  {id:<18} {kind:<5} (not directly settable — use select_*/presets)\n"
            )),
        }
        if verbose {
            if let Some(d) = agent::param_desc(id) {
                out.push_str(&format!("      {d}\n"));
            }
        }
    }
    let mut section = |title: &str, names: &[&'static str], desc: &dyn Fn(usize) -> &'static str| {
        out.push_str(&format!("\n{title}\n"));
        for (i, v) in names.iter().enumerate() {
            out.push_str(&format!("  {i:>2}  {v}\n"));
            if verbose {
                out.push_str(&format!("      {}\n", desc(i)));
            }
        }
    };
    section(
        "GENERATORS (`organon generator <name|ordinal>`):",
        &GeneratorMode::labels(),
        &generator_desc_at,
    );
    section(
        "SURFACES (`organon surface <name|ordinal>`):",
        &SurfaceMode::labels(),
        &surface_desc_at,
    );
    section(
        "MATERIALS (`organon material <name|ordinal>`):",
        &MaterialType::labels(),
        &material_desc_at,
    );
    out
}

/// `organon describe <query>` — the targeted knowledge lookup. Resolves `query` to a param
/// id (exact) → its kind/range/current + gloss; otherwise to any matching generator / surface
/// / material name-or-ordinal → the entity prose (a name shared across kinds, e.g. `original`,
/// prints every match, each labelled). `Err` lists no-match.
pub fn describe_text(s: Option<&Shared>, query: &str) -> Result<String, String> {
    let q = query.trim();
    // Additive: a query can name more than one thing (a recipe AND a generator, an
    // ordinal valid in all three enums, …). Show every match, labelled — the agent
    // gets the fullest answer rather than a single arbitrary winner.
    let mut out = String::new();
    // A recipe name → its full breakdown (Layer 3).
    if let Some(detail) = recipe_detail(q) {
        out.push_str(&detail);
    }
    // A settable param → its full card.
    if let Some((lo, hi)) = agent::id_range(q) {
        let kind = catalog_entries()
            .into_iter()
            .find(|(id, _, _)| *id == q)
            .map(|(_, k, _)| k)
            .unwrap_or("num");
        out.push_str(&format!("{q}  ({kind}, {lo} .. {hi})"));
        if let Some(v) = s.and_then(|s| agent::current(s, q)) {
            out.push_str(&format!("  = {v}"));
        }
        out.push('\n');
        out.push_str(&format!("  {}\n", agent::param_desc(q).unwrap_or("(no description)")));
    }
    // Any selector kind the name/ordinal matches.
    if let Ok(i) = resolve_enum::<GeneratorMode>(q) {
        let (i, name) = (i as usize, &GeneratorMode::labels()[i as usize]);
        out.push_str(&format!("GENERATOR {i}  {name}\n  {}\n", generator_desc_at(i)));
    }
    if let Ok(i) = resolve_enum::<SurfaceMode>(q) {
        let (i, name) = (i as usize, &SurfaceMode::labels()[i as usize]);
        out.push_str(&format!("SURFACE {i}  {name}\n  {}\n", surface_desc_at(i)));
    }
    if let Ok(i) = resolve_enum::<MaterialType>(q) {
        let (i, name) = (i as usize, &MaterialType::labels()[i as usize]);
        out.push_str(&format!("MATERIAL {i}  {name}\n  {}\n", material_desc_at(i)));
    }
    if out.is_empty() {
        Err(format!(
            "'{q}' is not a known param, generator, surface, material, or recipe — see \
             `organon catalog` / `organon recipes`"
        ))
    } else {
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// #452 Layer 3 — the recipe library (built-in described starting-points).
// ---------------------------------------------------------------------------

/// The full breakdown of one recipe (used by `describe` + `recipe --dry-run`).
pub fn recipe_detail(name: &str) -> Option<String> {
    let r = crate::recipe::recipe(name)?;
    let mut out = format!("RECIPE {}  \"{}\"\n  {}\n", r.name, r.title, r.intent);
    out.push_str("  apply: `organon recipe ");
    out.push_str(r.name);
    out.push_str("`\n");
    if let Some(g) = r.generator {
        out.push_str(&format!("    generator {g}\n"));
    }
    if let Some(sf) = r.surface {
        out.push_str(&format!("    surface   {sf}\n"));
    }
    if let Some(m) = r.material {
        out.push_str(&format!("    material  {m}\n"));
    }
    for (id, v) in r.params {
        out.push_str(&format!("    set {id} {v}\n"));
    }
    Some(out)
}

/// `organon recipes` — the library list (name · title · intent), text or JSON.
pub fn recipes_text(json: bool) -> String {
    if json {
        let arr: Vec<serde_json::Value> = crate::recipe::recipes()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "title": r.title,
                    "intent": r.intent,
                    "generator": r.generator,
                    "surface": r.surface,
                    "material": r.material,
                    "params": r.params.iter().map(|(id, v)| serde_json::json!({ "id": id, "value": v })).collect::<Vec<_>>(),
                })
            })
            .collect();
        return serde_json::Value::Array(arr).to_string();
    }
    let mut out = String::from("RECIPES (apply with `organon recipe <name>`):\n");
    for r in crate::recipe::recipes() {
        out.push_str(&format!("  {:<10} {}\n      {}\n", r.name, r.title, r.intent));
    }
    out
}

/// Translate a recipe into the command-channel ops that apply it: the generator / surface /
/// material selects followed by the param sets. `Err` if the name is unknown or (defensively)
/// a param/selector fails to resolve.
pub fn recipe_ops(name: &str) -> Result<Vec<CliOp>, String> {
    let r = crate::recipe::recipe(name).ok_or_else(|| {
        format!("no recipe '{name}' — see `organon recipes`")
    })?;
    let mut ops = Vec::new();
    if let Some(g) = r.generator {
        ops.push(CliOp::Generator(resolve_enum::<GeneratorMode>(g)?));
    }
    if let Some(sf) = r.surface {
        ops.push(CliOp::Surface(resolve_enum::<SurfaceMode>(sf)?));
    }
    if let Some(m) = r.material {
        ops.push(CliOp::Material(resolve_enum::<MaterialType>(m)?));
    }
    for (id, v) in r.params {
        if agent::id_range(id).is_none() {
            return Err(format!("recipe '{name}': '{id}' is not actuatable"));
        }
        ops.push(CliOp::Set((*id).to_string(), *v));
    }
    Ok(ops)
}

fn variant_name<E: IndexedEnum>(ordinal: u32) -> String {
    let vars = E::labels();
    vars.get(ordinal as usize)
        .map(|v| v.to_string())
        .unwrap_or_else(|| format!("?{ordinal}"))
}

/// One-shot status: selectors, tempo, transport — text form.
pub fn status_text(s: &Shared) -> String {
    let playing = s.transport[0] > 0.5;
    format!(
        "generator: {} \"{}\"   surface: {} \"{}\"   material: {} \"{}\"\n\
         tempo: {} bpm (sync {})   transport: {}, beat {:.2}{}\n",
        s.generator,
        variant_name::<GeneratorMode>(s.generator),
        s.surface_mode,
        variant_name::<SurfaceMode>(s.surface_mode),
        s.lighting[7] as u32,
        variant_name::<MaterialType>(s.lighting[7] as u32),
        s.tempo,
        if s.tempo_sync != 0 { "on" } else { "off" },
        if playing { "playing" } else { "stopped" },
        s.transport[1],
        if s.transport[3] > 0.5 {
            format!(" (host {} bpm)", s.transport[2])
        } else {
            String::new()
        },
    )
}

/// One-shot status as JSON.
pub fn status_json(s: &Shared) -> String {
    serde_json::json!({
        "generator": { "ordinal": s.generator, "name": variant_name::<GeneratorMode>(s.generator) },
        "surface": { "ordinal": s.surface_mode, "name": variant_name::<SurfaceMode>(s.surface_mode) },
        "material": { "ordinal": s.lighting[7] as u32, "name": variant_name::<MaterialType>(s.lighting[7] as u32) },
        "tempo_bpm": s.tempo,
        "tempo_sync": s.tempo_sync != 0,
        "playing": s.transport[0] > 0.5,
        "beat": s.transport[1],
        "host_tempo_bpm": if s.transport[3] > 0.5 { Some(s.transport[2]) } else { None },
    })
    .to_string()
}

/// `get` output — one id or all actuatable ids, text or JSON.
pub fn get_output(s: &Shared, id: Option<&str>, json: bool) -> Result<String, String> {
    match id {
        Some(id) => {
            let v = agent::current(s, id)
                .ok_or_else(|| format!("unknown/unreadable param '{id}' — see `organon catalog`"))?;
            Ok(if json {
                serde_json::json!({ id: v }).to_string()
            } else {
                format!("{v}")
            })
        }
        None => {
            let mut map = serde_json::Map::new();
            for id in agent::ACTUATABLE_IDS {
                if let Some(v) = agent::current(s, id) {
                    map.insert(id.to_string(), serde_json::json!(v));
                }
            }
            if json {
                Ok(serde_json::Value::Object(map).to_string())
            } else {
                Ok(map
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}\n"))
                    .collect::<String>())
            }
        }
    }
}

/// One `watch` tick: a compact JSON line — beat + the requested fields
/// (default: every actuatable id).
pub fn watch_line(s: &Shared, fields: &[String]) -> String {
    let mut map = serde_json::Map::new();
    map.insert("beat".into(), serde_json::json!(s.transport[1]));
    map.insert("playing".into(), serde_json::json!(s.transport[0] > 0.5));
    if fields.is_empty() {
        for id in agent::ACTUATABLE_IDS {
            if let Some(v) = agent::current(s, id) {
                map.insert(id.to_string(), serde_json::json!(v));
            }
        }
    } else {
        for f in fields {
            if let Some(v) = agent::current(s, f) {
                map.insert(f.clone(), serde_json::json!(v));
            }
        }
    }
    serde_json::Value::Object(map).to_string()
}

// ---------------------------------------------------------------------------
// The generated reference docs (`organon docs`)
// ---------------------------------------------------------------------------

/// Where the generated reference lands, relative to the repository root.
pub const DOCS_DIR: &str = "doc/reference";

/// Header stamped onto every generated file. It has to say *what regenerates it*,
/// because the whole point of generating is that nobody hand-edits the result and
/// then watches it drift back.
fn docs_banner(subject: &str) -> String {
    format!(
        "<!-- GENERATED BY `organon docs` — DO NOT EDIT BY HAND.\n     \
         The prose lives in the Rust source ({subject}); edit it there and re-run\n     \
         `cargo run --bin organon -- docs`. A test (`generated_reference_is_current`)\n     \
         fails if this file drifts from the code. -->\n\n"
    )
}

/// Render a range the way `catalog_text` does, so the CLI and the docs never
/// disagree about what a bound is.
fn range_md(r: Option<(f32, f32)>) -> String {
    match r {
        Some((lo, hi)) => format!("`{lo}` … `{hi}`"),
        None => "—".to_string(),
    }
}

/// A heading's anchor, by GitHub's rule: lowercase, drop punctuation other than
/// `-`/`_`, spaces become hyphens. Several selector names carry an en dash or
/// parentheses (`Frenet–Serret`, `L-system (plant)`), so a naive
/// replace-everything-with-hyphen would emit links that 404.
fn anchor(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// A selector name as a shell argument — quoted when it needs to be.
fn shell_arg(name: &str) -> String {
    let n = name.to_lowercase();
    if n.contains(' ') {
        format!("\"{n}\"")
    } else {
        n
    }
}

/// Collapse a Rust description into one line and neutralize the table delimiter,
/// so a gloss that grows a `|` cannot silently break the row it lives in.
fn cell(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "\\|")
}

/// One selector reference page (generators / surfaces / materials): an index table
/// followed by a section per variant carrying its compile-enforced prose.
fn selector_md(
    title: &str,
    lede: &str,
    subject: &str,
    command: &str,
    names: &[&'static str],
    desc: &dyn Fn(usize) -> &'static str,
) -> String {
    let mut out = docs_banner(subject);
    out.push_str(&format!("# {title}\n\n{lede}\n\n"));
    out.push_str("| # | Name | Select it with |\n|---|---|---|\n");
    for (i, n) in names.iter().enumerate() {
        out.push_str(&format!(
            "| {i} | [{}](#{}) | `organon {command} {i}` |\n",
            cell(n),
            anchor(n)
        ));
    }
    out.push('\n');
    for (i, n) in names.iter().enumerate() {
        out.push_str(&format!("## {n}\n\n{}\n\n", cell(desc(i))));
        out.push_str(&format!(
            "`organon {command} {i}` · `organon describe {}`\n\n",
            shell_arg(n)
        ));
    }
    out
}

/// The parameter reference: every id the CLI can set, with its kind, range and gloss.
fn parameters_md() -> String {
    let mut out = docs_banner("`native/src/agent.rs::param_desc` + `id_range`");
    out.push_str(
        "# Parameters\n\nThese are the controls the `organon` CLI can set directly, in raw \
         units:\n\n```bash\norganon set metallic 0.9 exposure -1.5\n```\n\nThe editor exposes \
         far more than this — every control on every card is a host parameter your DAW can \
         automate and MIDI-learn. This page covers the subset that has a stable command-line \
         id, which is the subset built for scripting.\n\n",
    );
    out.push_str("| Id | Kind | Range | What it does |\n|---|---|---|---|\n");
    for (id, kind, range) in catalog_entries() {
        let desc = agent::param_desc(id).map(cell).unwrap_or_else(|| "—".to_string());
        out.push_str(&format!(
            "| `{id}` | {kind} | {} | {desc} |\n",
            range_md(range)
        ));
    }
    out.push_str(
        "\nA row with no range is **chosen by name rather than set by value** — use the \
         matching selector (`organon generator …`, `organon surface …`, \
         `organon material …`) or recall a preset.\n",
    );
    out
}

/// The recipe reference: each built-in starting-point and exactly what it changes.
fn recipes_md() -> String {
    let mut out = docs_banner("`native/src/recipe.rs`");
    out.push_str(
        "# Recipes\n\nRecipes are described starting-points that ship **inside the binary**, \
         so a freshly-installed Organon with an empty preset store can still be driven to a \
         finished look. Applying one selects a generator, surface and material and sets a \
         handful of key parameters — a launch pad, not a ceiling.\n\n\
         ```bash\norganon recipes                 # list them\norganon recipe helix --dry-run  \
         # see what it would do\norganon recipe helix            # apply it\n```\n\n",
    );
    for r in crate::recipe::recipes() {
        out.push_str(&format!("## {} (`{}`)\n\n{}\n\n", r.title, r.name, cell(r.intent)));
        let sel = [("Generator", r.generator), ("Surface", r.surface), ("Material", r.material)];
        for (label, v) in sel {
            if let Some(v) = v {
                out.push_str(&format!("- **{label}:** {v}\n"));
            }
        }
        if !r.params.is_empty() {
            out.push_str("- **Sets:** ");
            let sets: Vec<String> =
                r.params.iter().map(|(id, v)| format!("`{id}` = {v}")).collect();
            out.push_str(&sets.join(", "));
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// The index page for `doc/reference/`.
fn reference_index_md() -> String {
    let mut out = docs_banner("`native/src/cli.rs`");
    out.push_str(
        "# Reference\n\nEvery page here is **generated from the source** by \
         `cargo run --bin organon -- docs`, and a test fails the build if a checked-in page \
         drifts from the code. Fix a description by editing the Rust, not the Markdown.\n\n\
         | Page | What it covers | Source of truth |\n|---|---|---|\n\
         | [Generators](generators.md) | the geometry engines — what each one makes | \
         `agent.rs::generator_desc` |\n\
         | [Surfaces](surfaces.md) | how a generator's nodes become geometry | \
         `agent.rs::surface_desc` |\n\
         | [Materials](materials.md) | how that geometry is shaded | \
         `agent.rs::material_desc` |\n\
         | [Parameters](parameters.md) | every CLI-settable control, with ranges | \
         `agent.rs::param_desc` |\n\
         | [Recipes](recipes.md) | built-in starting-points | `recipe.rs` |\n\n\
         For the narrative version — what to *do* with these — start at \
         [the guide](../guide/README.md).\n\n\
         The `organon` CLI serves this same material from the same source, so the two \
         cannot disagree; it is simply closer to hand when you are already at a terminal, \
         and it works whether or not Organon is running:\n\n\
         ```bash\norganon catalog --manual     # all of it, in the terminal\n\
         organon describe dna         # one entry in depth\n```\n",
    );
    out
}

/// Does a checked-out page still match what the code would emit?
///
/// ⚠️ **Compare CONTENT, not bytes — the difference is a platform, not a nicety.**
/// The repository's `.gitattributes` sets `* text=auto`, so Markdown is stored LF and
/// checked out **CRLF on Windows**, while [`docs_files`] always emits LF. A byte-exact
/// comparison therefore passes on every Linux leg and fails on Windows for *every* file,
/// which is exactly how this shipped: three green Linux legs and a red `build (windows)`.
/// `ci.yml`'s header already warns that the Windows checkout arrives CRLF and calls it
/// "fine for Rust and WGSL" — true, and not true for a comparison.
///
/// The fix belongs here rather than in `.gitattributes`. Pinning these files to LF would
/// also turn the leg green, but it would change the repository's checkout policy to suit
/// a test, and that file's own rule is that each pin exists for a specific failure. CRLF
/// Markdown is not a failure; a test that cannot read it is.
pub fn docs_match(on_disk: &str, generated: &str) -> bool {
    on_disk.replace("\r\n", "\n") == generated
}

/// Every generated reference page as `(filename, contents)`. Pure — it reads no
/// running app and no filesystem, so it runs offline, in CI, and inside a test.
pub fn docs_files() -> Vec<(&'static str, String)> {
    vec![
        ("README.md", reference_index_md()),
        (
            "generators.md",
            selector_md(
                "Generators",
                "The generator is the engine that builds the geometry — it decides *what shape* \
                 you are looking at. Everything downstream (surface, material, lighting, camera, \
                 beat) works the same way whichever one you pick.",
                "`native/src/agent.rs::generator_desc`",
                "generator",
                &GeneratorMode::labels(),
                &generator_desc_at,
            ),
        ),
        (
            "surfaces.md",
            selector_md(
                "Surfaces",
                "The surface mode decides how a generator's nodes become drawable geometry — \
                 cubes, tubes, a fused skin, a glowing cloud. It is orthogonal to the generator: \
                 any surface works with any node-field generator. The raymarched generators \
                 (Mandelbulb, the kaleidoscopic fractal, Lens and Creature) emit no nodes, so \
                 surface modes have nothing to act on there; Minimal Surfaces and Neural Field \
                 are dual-path, raymarching their implicit families and emitting a skinnable \
                 node grid for their parametric ones.",
                "`native/src/agent.rs::surface_desc`",
                "surface",
                &SurfaceMode::labels(),
                &surface_desc_at,
            ),
        ),
        (
            "materials.md",
            selector_md(
                "Materials",
                "The material decides how the geometry is shaded. It is orthogonal to both the \
                 generator and the surface, and it applies to the raymarched generators too.",
                "`native/src/agent.rs::material_desc`",
                "material",
                &MaterialType::labels(),
                &material_desc_at,
            ),
        ),
        ("parameters.md", parameters_md()),
        ("recipes.md", recipes_md()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn set_pairs_and_watch_fields_validate() {
        assert_eq!(
            pairs_from(&args(&["metallic", "0.9", "glow", "1.5"])).unwrap(),
            vec![("metallic".to_string(), 0.9), ("glow".to_string(), 1.5)]
        );
        assert!(pairs_from(&args(&["metallic"])).is_err()); // odd
        assert!(pairs_from(&args(&[])).is_err()); // empty
        assert!(pairs_from(&args(&["nonsense", "1"])).is_err()); // unknown id
        assert!(pairs_from(&args(&["metallic", "abc"])).is_err()); // not a number
        assert!(validate_fields(&args(&["glow", "metallic"])).is_ok());
        assert!(validate_fields(&args(&["nonsense"])).is_err());
    }

    #[test]
    fn resolves_selectors_by_ordinal_name_and_substring() {
        assert_eq!(resolve_enum::<GeneratorMode>("0"), Ok(0));
        assert_eq!(resolve_enum::<GeneratorMode>("dna").unwrap(), {
            GeneratorMode::Dna.to_u32()
        });
        assert_eq!(
            resolve_enum::<MaterialType>("chrome").unwrap(),
            MaterialType::Chrome.to_u32()
        );
        // Substring must be unambiguous.
        assert!(resolve_enum::<GeneratorMode>("neural").is_err()); // field vs network
        assert!(resolve_enum::<GeneratorMode>("zzz").is_err());
        assert!(resolve_enum::<GeneratorMode>("9999").is_err());
    }

    #[test]
    fn write_commands_become_channel_ops() {
        let ops = ops_for(&CtlCmd::Set {
            pairs: vec![("glow".into(), 1.5)],
        })
        .unwrap()
        .unwrap();
        assert_eq!(ops, vec![CliOp::Set("glow".into(), 1.5)]);
        let ops = ops_for(&CtlCmd::Material { which: "glass".into() }).unwrap().unwrap();
        assert_eq!(ops, vec![CliOp::Material(MaterialType::Glass.to_u32())]);
        assert_eq!(ops_for(&CtlCmd::Status { json: false }).unwrap(), None);
        assert!(ops_for(&CtlCmd::Generator { which: "zzz".into() }).is_err());
    }

    #[test]
    fn plan_normalizes_to_one_line_the_executor_accepts() {
        let multi = "{\n  \"name\": \"warm\",\n  \"moves\": [\n    {\"op\":\"set_param\",\"id\":\"glow\",\"value\":1.0}\n  ]\n}";
        let one = normalize_plan(multi).unwrap();
        assert!(!one.contains('\n'));
        assert!(agent::PhrasePlan::parse(&one).is_some());
        assert!(normalize_plan("not json").is_err());
    }

    #[test]
    fn catalog_covers_every_actuatable_id_and_the_selectors() {
        let entries = catalog_entries();
        for id in agent::ACTUATABLE_IDS {
            assert!(entries.iter().any(|(i, _, r)| i == id && r.is_some()), "{id} missing");
        }
        // No duplicate ids.
        let mut seen = std::collections::BTreeSet::new();
        for (id, _, _) in &entries {
            assert!(seen.insert(*id), "duplicate catalog id {id}");
        }
        let j: serde_json::Value = serde_json::from_str(&catalog_json(None)).unwrap();
        assert!(j["params"].as_array().unwrap().len() >= agent::ACTUATABLE_IDS.len());
        assert_eq!(
            j["generators"].as_array().unwrap().len(),
            GeneratorMode::labels().len()
        );
        assert!(!j["surfaces"].as_array().unwrap().is_empty());
        assert!(j["materials"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn read_formatters_speak_valid_json_over_a_default_snapshot() {
        let s = Shared::default();
        let st: serde_json::Value = serde_json::from_str(&status_json(&s)).unwrap();
        assert_eq!(st["generator"]["ordinal"], 0);
        assert_eq!(st["generator"]["name"], "Organic Math (cube field)");
        let g: serde_json::Value =
            serde_json::from_str(&get_output(&s, Some("metallic"), true).unwrap()).unwrap();
        assert!(g["metallic"].is_number());
        assert!(get_output(&s, Some("nonsense"), true).is_err());
        let all: serde_json::Value =
            serde_json::from_str(&get_output(&s, None, true).unwrap()).unwrap();
        assert!(all.as_object().unwrap().len() >= 40);
        let w: serde_json::Value =
            serde_json::from_str(&watch_line(&s, &["glow".into()])).unwrap();
        assert!(w["beat"].is_number());
        assert!(w["glow"].is_number());
        // Human text forms render without panicking and mention the selectors.
        assert!(catalog_text(Some(&s), false).contains("GENERATORS"));
        assert!(status_text(&s).contains("generator: 0"));
    }

    #[test]
    fn describe_resolves_params_and_selectors_with_prose() {
        let s = Shared::default();
        // A param → kind/range/current + its gloss.
        let d = describe_text(Some(&s), "metallic").unwrap();
        assert!(d.contains("metallic") && d.contains("(num,") && d.to_lowercase().contains("metal"));
        // A material name → the material prose.
        let d = describe_text(Some(&s), "glass").unwrap();
        assert!(d.contains("MATERIAL") && d.to_lowercase().contains("refract"));
        // A pure generator name (not also a recipe) → the generator prose.
        let d = describe_text(None, "strange").unwrap();
        assert!(d.contains("GENERATOR") && d.to_lowercase().contains("attractor"));
        // A surface name → the surface prose.
        assert!(describe_text(None, "metaball").unwrap().contains("SURFACE"));
        // A name that is BOTH a recipe and a generator substring shows both (additive).
        let d = describe_text(None, "dna").unwrap();
        assert!(d.contains("RECIPE dna") && d.contains("GENERATOR"));
        // An ordinal that is valid in every kind shows all three (multi-match).
        let d = describe_text(None, "0").unwrap();
        assert!(d.contains("GENERATOR") && d.contains("SURFACE") && d.contains("MATERIAL"));
        // A recipe name → its breakdown (Layer 3), via the same `describe`.
        let d = describe_text(None, "helix").unwrap();
        assert!(d.contains("RECIPE helix") && d.contains("generator") && d.contains("set ior"));
        // Unknown → error.
        assert!(describe_text(None, "nonsense_xyz").is_err());

        // Verbose catalog inlines the descriptions; JSON carries `desc` on every entry.
        let v = catalog_text(Some(&s), true);
        assert!(v.contains("metallic") && v.to_lowercase().contains("mirror-sharp"));
        let j: serde_json::Value = serde_json::from_str(&catalog_json(Some(&s))).unwrap();
        assert!(j["params"][0]["desc"].is_string() || j["params"][0]["desc"].is_null());
        assert!(j["generators"][0]["desc"].is_string());
        assert!(j["generators"][0]["name"].is_string());
    }

    #[test]
    fn recipes_list_and_apply_to_valid_ops() {
        // The list mentions a known recipe (text + JSON).
        assert!(recipes_text(false).contains("helix"));
        let j: serde_json::Value = serde_json::from_str(&recipes_text(true)).unwrap();
        assert!(j.as_array().unwrap().iter().any(|r| r["name"] == "helix"));

        // Applying resolves to selects + sets, all valid.
        let ops = recipe_ops("helix").unwrap();
        assert!(matches!(ops[0], CliOp::Generator(_)));
        assert!(ops.iter().any(|o| matches!(o, CliOp::Set(id, _) if id == "ior")));
        // Every op round-trips through the wire format the visual drains.
        for op in &ops {
            assert!(CliOp::parse(&op.to_line()).is_some(), "op {op:?} won't parse back");
        }
        // Unknown recipe → error, not a panic.
        assert!(recipe_ops("no_such_recipe").is_err());
    }

    /// The anti-rot gate. `doc/reference/` is generated from the prose in `agent.rs` and
    /// `recipe.rs`; this fails the build the moment the checked-in Markdown stops matching
    /// what the code would emit — which is what stops a shipped reference from quietly
    /// describing a version of Organon that no longer exists. Because `docs_files()` is
    /// pure, this needs no GPU, no running app and no network, so it runs everywhere
    /// `cargo test --workspace` does.
    #[test]
    fn generated_reference_is_current() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("native/ has a parent")
            .join(DOCS_DIR);
        for (name, want) in docs_files() {
            let path = root.join(name);
            let got = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "{} is missing ({e}). Regenerate it: cargo run --bin organon -- docs",
                    path.display()
                )
            });
            assert!(
                docs_match(&got, &want),
                "{} is stale. Regenerate it: cargo run --bin organon -- docs",
                path.display()
            );
        }
    }

    // -----------------------------------------------------------------------
    // The range tables, pinned to the engine
    // -----------------------------------------------------------------------

    /// The engine's own answer for every id the two hand-written range tables speak for:
    /// `params.rs` field name → `(min, max)`.
    ///
    /// **No bound is restated here** — every number is read off the real param object, which
    /// is the whole point: a third hand-written copy would just be a third thing to drift.
    /// The one hand-written part is the id → field *join*, and the compiler checks that. Every
    /// arm below touches `p.<field>` at a declared type, so a renamed field, a retyped param
    /// or a mis-paired enum is a build error rather than a silent disagreement.
    ///
    /// `OrganicMathParams::default()` constructs all 1372 host params with no host, no audio
    /// thread and no GPU, so reading them is as headless as the tables being checked.
    fn engine_ranges() -> std::collections::BTreeMap<&'static str, (f32, f32)> {
        use crate::params::{HostCamPath, HostFuncName, OrganicMathParams};
        use nih_plug::prelude::{BoolParam, Enum, EnumParam, FloatParam, IntParam, Param};

        let p = OrganicMathParams::default();
        let mut m = std::collections::BTreeMap::new();

        /// A float's range IS `preview_plain` at the two ends of normalized space.
        macro_rules! float {
            ($($f:ident),* $(,)?) => {$({
                let q: &FloatParam = &p.$f;
                m.insert(stringify!($f), (q.preview_plain(0.0), q.preview_plain(1.0)));
            })*};
        }
        /// Same for an int — the actuation lane carries every id as an `f32` regardless.
        macro_rules! int {
            ($($f:ident),* $(,)?) => {$({
                let q: &IntParam = &p.$f;
                m.insert(
                    stringify!($f),
                    (q.preview_plain(0.0) as f32, q.preview_plain(1.0) as f32),
                );
            })*};
        }
        /// A bool carries no range of its own; the lane spells it 0/1.
        macro_rules! boolean {
            ($($f:ident),* $(,)?) => {$({
                let _: &BoolParam = &p.$f;
                m.insert(stringify!($f), (0.0, 1.0));
            })*};
        }
        /// An enum is an `IntParam` over variant INDICES, so its top is `variants() - 1` —
        /// exactly the bound `cam_path` got wrong, by one, in the direction that admits a
        /// variant which does not exist.
        macro_rules! choice {
            ($($f:ident: $e:ty),* $(,)?) => {$({
                let _: &EnumParam<$e> = &p.$f;
                m.insert(stringify!($f), (0.0, (<$e as Enum>::variants().len() - 1) as f32));
            })*};
        }

        float!(
            rot_amp_x, rot_amp_y, rot_amp_z, rot_mod_x, rot_mod_y, rot_mod_z, //
            trans_amp_x, trans_amp_y, trans_amp_z, trans_mod_x, trans_mod_y, trans_mod_z,
            scale_amp, //
            ambient, key_intensity, fill_intensity, elevation, azimuth, glow, opacity, //
            metallic, roughness, exposure, env_intensity, env_rotation, bloom_intensity,
            bloom_threshold, ior, //
            subsurface, sss_distortion, sss_power, iridescence, irid_scale, irid_shift, //
            cam_speed, cam_kick, cam_damping, mat_hue, tempo, //
            // organon#217 T3 — the PBR text look, its held camera, the capsule core.
            glyph_cell_w, glyph_depth, glyph_gap, glyph_gain, glyph_faceplate, //
            glyph_back_r, glyph_back_g, glyph_back_b, glyph_margin, glyph_back_depth,
            glyph_default_fg, glyph_bevel, glyph_crown, glyph_profile, // (T9: profile)
            glyph_cam_tilt, glyph_cam_zoom, capsule_core, capsule_absorb,
            // organon#217 T13 / #240 — the halation and the glyph-lights.
            hal_amount, ml_intensity, ml_radius,
        );
        int!(loop_count_x, loop_count_y, loop_count_z, loop_count_q, ml_count);
        boolean!(
            bell_physical, animate, pulse, glyph_cam_hold, glyph_dark_tiles, //
            // organon#217 T13 / #240 — the dark room's switches.
            atmos_enabled, bg_visible, fx_enabled, ml_enabled, ml_restir,
        );
        choice!(
            cam_path: HostCamPath,
            rot_func: HostFuncName,
            trans_func: HostFuncName,
            scale_func: HostFuncName,
        );
        m
    }

    /// `clip::RANGES` slot → the `params.rs` field whose value that slot carries, in the
    /// canonical CC order `clip::get`/`clip::set` route. `None` marks a slot **no single param
    /// backs**; both are argued at the assertion below rather than waved through here.
    const CLIP_SLOT_FIELDS: [Option<&str>; crate::clip::N] = [
        Some("loop_count_x"), Some("loop_count_y"), Some("loop_count_z"), Some("loop_count_q"),
        Some("rot_amp_x"), Some("rot_amp_y"), Some("rot_amp_z"),
        Some("rot_mod_x"), Some("rot_mod_y"), Some("rot_mod_z"),
        Some("trans_amp_x"), Some("trans_amp_y"), Some("trans_amp_z"),
        Some("trans_mod_x"), Some("trans_mod_y"), Some("trans_mod_z"),
        None, // 16 — the effective-speed EXPRESSION slot, not a param
        Some("scale_amp"),
        Some("ambient"), Some("key_intensity"), Some("fill_intensity"),
        Some("elevation"), Some("azimuth"), Some("glow"), Some("opacity"),
        Some("tempo"),
        None, // 26 — reserved/inert; `to_shared` hard-codes 0.0, no param exists
        Some("rot_func"), Some("trans_func"), Some("scale_func"),
        Some("animate"), Some("pulse"),
    ];

    /// The gate: **the hand-written range tables equal the engine's ranges**, and the engine's
    /// taper is the linear law those tables assume.
    ///
    /// `agent::id_range` and `clip::RANGES` are hand-written mirrors of `params.rs`. Nothing
    /// pinned them and they drifted — 9 of the 45 actuatable ids were wrong when this test was
    /// written: `trans_amp_x/y/z` by **10× on the maximum** (which the published
    /// `doc/reference/parameters.md` shipped to readers), plus `exposure`, `bloom_intensity`,
    /// `sss_power`, `irid_scale`, `cam_damping`, and `cam_path`, whose top admitted a 12th
    /// `CamPath` variant that does not exist. An agent told a param runs to 200 when it stops
    /// at 20 gets no error — it gets a silent clamp and a look it did not ask for, and
    /// `recipe.rs` was validating against the same wrong bounds. That is the failure mode a
    /// mirror with no mirror-check always ends in, so the fix is not "be careful": it is this.
    ///
    /// Two claims, one test, because they are one claim from both ends:
    ///
    /// 1. **The tables equal the engine** — every [`agent::ACTUATABLE_IDS`] entry and every
    ///    [`crate::clip::RANGES`] slot, against the param object that owns the bound.
    /// 2. **The engine's taper is linear**, over all ~1372 host params — the law the tables,
    ///    the CC lane's `apply_normalized`, and any descriptor built on a `(min, max)` pair
    ///    all assume when they treat a range as two numbers. `FloatRange::Linear` is hard-coded
    ///    in `params.rs::flin` today; the day someone reaches for `Skewed`, two numbers stop
    ///    describing the parameter, and this fails instead of letting the tables lie again.
    ///
    /// Headless by construction, so it runs everywhere `cargo test --workspace` does.
    #[test]
    fn taper_round_trips_against_the_engine_range() {
        use crate::params::OrganicMathParams;
        use nih_plug::prelude::{Param, ParamPtr, Params};

        let engine = engine_ranges();
        let mut wrong: Vec<String> = Vec::new();

        // --- 1. `agent::id_range` ------------------------------------------------------
        for id in agent::ACTUATABLE_IDS {
            let want = *engine.get(id).unwrap_or_else(|| {
                panic!("{id} is actuatable but has no `engine_ranges` join — add one")
            });
            let got = agent::id_range(id).unwrap_or_else(|| panic!("{id} has no range"));
            if got != want {
                wrong.push(format!("agent::id_range {id}: table {got:?}, engine {want:?}"));
            }
        }

        // --- 2. `clip::RANGES` ---------------------------------------------------------
        //
        // Two slots have no param behind them, and each is exempt for a stated reason, not
        // because checking them was inconvenient:
        //
        // * **16 — effective global speed.** The slot carries `rot_mod[3]`, which
        //   `param_table.rs` packs as the EXPRESSION `inc_scale × 10^speed_exp`, not as a
        //   parameter. Its `(0, 0.1)` is a deliberate playable-range choice for the CC lane
        //   (the product's default is 0.01 and its ceiling is 1.0, so a full-span CC would
        //   spend 90% of its travel past anything usable) — see the comment at the table.
        // * **26 — reserved.** The Pulse Depth knob was removed; `params.rs::to_shared`
        //   hard-codes `pulse_depth: 0.0` and no param exists to check against.
        //
        // A slot gaining a param must therefore be *joined here*, not left as `None`.
        for (i, field) in CLIP_SLOT_FIELDS.iter().enumerate() {
            let Some(field) = field else { continue };
            let want = *engine.get(field).unwrap_or_else(|| {
                panic!("clip slot {i} names `{field}`, which `engine_ranges` has no join for")
            });
            let got = crate::clip::RANGES[i];
            if got != want {
                wrong.push(format!(
                    "clip::RANGES[{i}] ({field}): table {got:?}, engine {want:?}"
                ));
            }
        }

        assert!(
            wrong.is_empty(),
            "{} range table entries disagree with `params.rs`:\n  {}",
            wrong.len(),
            wrong.join("\n  ")
        );

        // --- 3. The engine's taper, over every host param -------------------------------
        let p = OrganicMathParams::default();
        for (wire_id, ptr, _group) in p.param_map() {
            // SAFETY: `Params::param_map`'s contract — the pointers are valid for as long as
            // the object they came from is, and `p` outlives this loop.
            unsafe {
                match ptr {
                    ParamPtr::FloatParam(q) => {
                        let q = &*q;
                        let (lo, hi) = (q.preview_plain(0.0), q.preview_plain(1.0));
                        assert!(lo <= hi, "{wire_id}: min {lo} > max {hi}");
                        let d = q.default_plain_value();
                        assert!(d >= lo && d <= hi, "{wire_id}: default {d} outside {lo}..{hi}");
                        for step in 0..=20 {
                            let n = step as f32 / 20.0;
                            let want = lo + n * (hi - lo);
                            let got = q.preview_plain(n);
                            let tol = (want.abs().max(1.0)) * 1.0e-5;
                            assert!(
                                (got - want).abs() <= tol,
                                "{wire_id} is not linear: preview_plain({n}) = {got}, \
                                 linear law says {want}"
                            );
                        }
                    }
                    // An int (and an enum, which is an int over variant indices) rounds to the
                    // nearest step, so the law is the linear one THEN `.round()`.
                    ParamPtr::IntParam(q) => check_int(&*q, &wire_id),
                    ParamPtr::EnumParam(q) => check_int(&*q, &wire_id),
                    // A bool has no range to round-trip.
                    ParamPtr::BoolParam(_) => {}
                }
            }
        }

        fn check_int<P: Param<Plain = i32>>(q: &P, wire_id: &str) {
            let (lo, hi) = (q.preview_plain(0.0), q.preview_plain(1.0));
            assert!(lo <= hi, "{wire_id}: min {lo} > max {hi}");
            let d = q.default_plain_value();
            assert!(d >= lo && d <= hi, "{wire_id}: default {d} outside {lo}..{hi}");
            for step in 0..=20 {
                let n = step as f32 / 20.0;
                let want = (n * (hi - lo) as f32).round() as i32 + lo;
                let got = q.preview_plain(n);
                assert_eq!(
                    got, want,
                    "{wire_id} is not linear: preview_plain({n}) = {got}, linear law says {want}"
                );
            }
        }
    }

    /// Every generator, surface and material reaches the docs, and every one of them
    /// carries real prose. The `match`es in `agent.rs` are exhaustive, so a new variant
    /// cannot compile undescribed — but it *could* land with a placeholder, and a
    /// reference page full of "TODO" is worse than one that is missing.
    #[test]
    fn every_selector_variant_is_documented() {
        let files = docs_files();
        let page = |n: &str| &files.iter().find(|(f, _)| *f == n).expect("page exists").1;
        for (names, file) in [
            (&GeneratorMode::labels(), "generators.md"),
            (&SurfaceMode::labels(), "surfaces.md"),
            (&MaterialType::labels(), "materials.md"),
        ] {
            let md = page(file);
            for n in names {
                assert!(md.contains(&format!("## {n}\n")), "{file} is missing {n}");
            }
        }
        for i in 0..GeneratorMode::labels().len() {
            let d = generator_desc_at(i);
            assert!(d.len() > 40, "generator {i} has a stub description: {d:?}");
        }
    }

    /// A CRLF checkout of a generated page still counts as current.
    ///
    /// This is the `build (windows)` failure as a unit test, and it has to be one: the
    /// defect is invisible to every Linux leg by construction, so without this the only
    /// thing standing between it and `main` is a Windows runner nobody is required to
    /// look at. Asserting both directions also pins the asymmetry — the on-disk side is
    /// normalized, the generated side is not, because `docs_files()` emits LF and a
    /// generator that started emitting CRLF would be a real bug worth failing on.
    #[test]
    fn a_crlf_checkout_is_not_drift() {
        let (_, want) = &docs_files()[0];
        let crlf = want.replace('\n', "\r\n");
        assert_ne!(crlf, *want, "the fixture must actually differ byte-wise");
        assert!(docs_match(&crlf, want), "a CRLF checkout must read as current");
        assert!(docs_match(want, want), "an LF checkout must read as current");
        assert!(
            !docs_match("# Something else\n", want),
            "genuinely different content must still read as drift"
        );
    }

    /// Every id that reaches the published reference carries a gloss.
    ///
    /// `every_actuatable_id_has_a_gloss` guards `ACTUATABLE_IDS`, but the docs table is
    /// built from `catalog_entries()` — the UNION of that set and `core_catalog()`. Three
    /// ids sat in the gap (`continuous`, `mat_type`, `palette`) and shipped as a row with
    /// an em dash in both the range and the meaning column, which reads as an unfinished
    /// document rather than as the deliberate "chosen by name, not by value" that it is.
    /// Guarding the union is what stops the next `core_catalog()` addition doing it again.
    #[test]
    fn every_documented_param_has_a_gloss() {
        let missing: Vec<&str> = catalog_entries()
            .iter()
            .filter(|(id, _, _)| agent::param_desc(id).is_none())
            .map(|(id, _, _)| *id)
            .collect();
        assert!(
            missing.is_empty(),
            "these ids reach doc/reference/parameters.md with no description — add one to \
             `agent::param_desc`: {missing:?}"
        );
    }

    // -----------------------------------------------------------------------
    // #147 Tier 3½ — the Delta lens's adapter.
    //
    // ⚠️ **No test here touches `ipc::adapter_sidecar_path()`**, and that is
    // deliberate rather than incidental: it is a real file in `$TMPDIR` that a
    // running Organon Mind reads, so a test writing it would clear whichever
    // adapter the person running `cargo test` had selected. Every sidecar in this
    // module is a file inside the test's own scratch directory, which is what
    // `select_adapter` / `clear_adapter` taking the path as a parameter buys.
    // -----------------------------------------------------------------------

    /// A scratch directory under `target/` — gitignored, and **not** in `$TMPDIR`,
    /// where the real sidecar lives.
    ///
    /// Returns `(relative, absolute)`. The relative half is what makes
    /// `a_relative_directory_is_written_absolute` possible without
    /// `set_current_dir`, which is process-global and would race every other test
    /// in this binary.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let rel = PathBuf::from("target").join("adapter-fixtures").join(name);
        let _ = std::fs::remove_dir_all(&rel);
        std::fs::create_dir_all(&rel).expect("scratch directory");
        let abs = std::fs::canonicalize(&rel).expect("canonicalize scratch");
        (rel, PathBuf::from(de_verbatim(abs.to_str().unwrap())))
    }

    /// The smallest thing that is genuinely a safetensors file: an 8-byte LE header
    /// length, that many bytes of JSON, then the payload the offsets promise. One
    /// `lora_A`/`lora_B` pair of 2×2 `F32`, so `lora::read_adapter_dir` accepts it
    /// too — pinned by `the_fixture_is_an_adapter_lora_itself_accepts`.
    fn safetensors_pair() -> Vec<u8> {
        let stem = "base_model.model.model.layers.0.self_attn.q_proj";
        let header = format!(
            "{{\"{stem}.lora_A.weight\":{{\"dtype\":\"F32\",\"shape\":[2,2],\
             \"data_offsets\":[0,16]}},\"{stem}.lora_B.weight\":{{\"dtype\":\"F32\",\
             \"shape\":[2,2],\"data_offsets\":[16,32]}}}}"
        );
        let mut out = (header.len() as u64).to_le_bytes().to_vec();
        out.extend_from_slice(header.as_bytes());
        for _ in 0..8 {
            out.extend_from_slice(&1.0f32.to_le_bytes());
        }
        out
    }

    const GOOD_CONFIG: &str = r#"{"peft_type":"LORA","r":2,"lora_alpha":4,
        "target_modules":["q_proj","v_proj"],
        "base_model_name_or_path":"unsloth/tiny-fixture"}"#;

    /// Lay a complete, readable adapter down in `dir`.
    fn write_adapter(dir: &Path) {
        std::fs::write(dir.join(ADAPTER_CONFIG), GOOD_CONFIG).unwrap();
        std::fs::write(dir.join(ADAPTER_WEIGHTS), safetensors_pair()).unwrap();
    }

    /// **A Windows `canonicalize` returns a verbatim path, and only a drive-letter
    /// one may be shortened.**
    ///
    /// Pure and over `&str` on purpose: no path on Linux CI is ever verbatim, so a
    /// `Path`-shaped version of this would compile there and never execute either
    /// arm — the shape of "green on the machine that cannot run it" this repo keeps
    /// paying for.
    ///
    /// ⚠️ **Mutation-tested.** Replacing the UNC guard with the tempting
    /// `strip_prefix(r"\\?\UNC\")` fails here at the share case: `\\server\share`
    /// becomes `server\share`, a **relative** path, which is the single failure the
    /// whole absolutising step exists to prevent.
    #[test]
    fn de_verbatim_shortens_a_drive_path_and_leaves_everything_else_alone() {
        assert_eq!(de_verbatim(r"\\?\C:\models\lora-r16"), r"C:\models\lora-r16");
        assert_eq!(de_verbatim(r"\\?\c:\x"), r"c:\x");
        // A UNC share: shortening it correctly means `\\server\share`, and the
        // obvious `strip_prefix` yields a relative path instead. Left alone.
        assert_eq!(de_verbatim(r"\\?\UNC\server\share\lora"), r"\\?\UNC\server\share\lora");
        // A volume GUID has no `:` in second position — not a drive path, left alone.
        assert_eq!(
            de_verbatim(r"\\?\Volume{b75e2c83}\lora"),
            r"\\?\Volume{b75e2c83}\lora"
        );
        // Everything Linux and macOS ever produce.
        assert_eq!(de_verbatim("/home/james/lora"), "/home/james/lora");
        assert_eq!(de_verbatim(""), "");
    }

    /// 🚨 **A relative directory comes back absolute.** The visual is a different
    /// process with a different working directory, so a relative path in the sidecar
    /// resolves somewhere else or nowhere — and the failure is a lens that clears
    /// the graph while the file looks perfectly reasonable to whoever wrote it.
    ///
    /// ⚠️ **Mutation-tested.** Returning `dir.to_path_buf()` from
    /// `check_adapter_dir` instead of the canonicalized path fails here at
    /// *"the written path must be absolute"*.
    #[test]
    fn a_relative_directory_is_written_absolute() {
        let (rel, abs) = scratch("relative");
        write_adapter(&rel);
        assert!(!rel.is_absolute(), "the fixture must be handed over relative");
        let got = check_adapter_dir(&rel).expect("a complete adapter directory");
        assert!(got.is_absolute(), "the written path must be absolute: {}", got.display());
        assert_eq!(got, abs);
    }

    /// The fixture is an adapter `lora` itself reads, so "this check accepts it" and
    /// "the lens can use it" are the same sentence for at least one directory.
    #[test]
    fn the_fixture_is_an_adapter_lora_itself_accepts() {
        let (rel, _) = scratch("lora-accepts");
        write_adapter(&rel);
        let summary = organon_core::lora::read_adapter_dir(&rel)
            .expect("`lora::read_adapter_dir` must accept the fixture");
        assert_eq!(summary.modules.len(), 1);
    }

    /// Each refusal names what is wrong with **this** directory. The four structural
    /// ones, one fixture each.
    ///
    /// ⚠️ **Mutation-tested.** Dropping the `meta.is_dir()` arm makes the file case
    /// fall through to `NoMember`, failing here at *"a file is not a directory"*.
    #[test]
    fn each_structural_refusal_names_what_is_wrong_with_this_directory() {
        let (root, _) = scratch("refusals");

        // Missing.
        let gone = root.join("not-here");
        match check_adapter_dir(&gone) {
            Err(AdapterRefusal::Missing { .. }) => {}
            other => panic!("a missing path must be Missing, got {other:?}"),
        }

        // A file, not a directory — picking `adapter_model.safetensors` itself is
        // the likely mistake, so the sentence has to say which of the two it wanted.
        let as_file = root.join("a-file");
        std::fs::write(&as_file, b"not a directory").unwrap();
        match check_adapter_dir(&as_file) {
            Err(e @ AdapterRefusal::NotADirectory { .. }) => {
                assert!(e.sentence().contains(ADAPTER_CONFIG), "{}", e.sentence());
            }
            other => panic!("a file is not a directory, got {other:?}"),
        }

        // No config.
        let no_cfg = root.join("no-config");
        std::fs::create_dir_all(&no_cfg).unwrap();
        std::fs::write(no_cfg.join(ADAPTER_WEIGHTS), safetensors_pair()).unwrap();
        match check_adapter_dir(&no_cfg) {
            Err(AdapterRefusal::NoMember { name, .. }) => assert_eq!(name, ADAPTER_CONFIG),
            other => panic!("a directory with no config must name it, got {other:?}"),
        }

        // No weights.
        let no_w = root.join("no-weights");
        std::fs::create_dir_all(&no_w).unwrap();
        std::fs::write(no_w.join(ADAPTER_CONFIG), GOOD_CONFIG).unwrap();
        match check_adapter_dir(&no_w) {
            Err(AdapterRefusal::NoMember { name, .. }) => assert_eq!(name, ADAPTER_WEIGHTS),
            other => panic!("a directory with no weights must name it, got {other:?}"),
        }
    }

    /// 🚨 **Both members are opened and judged by `lora`'s own parsers**, so the CLI
    /// cannot come to a different verdict from the reader it is feeding.
    ///
    /// The two cases are the ones a person actually hits. **DoRA** is refused by name
    /// — its update is not `(alpha/r)·B·A`, so reading it as LoRA yields plausible
    /// numbers rather than an error. And a **git-lfs pointer** is what a `git clone`
    /// of a HuggingFace repo leaves behind when lfs is not installed: a ~130-byte
    /// text file with the right name, which exists and opens and is not an adapter.
    ///
    /// ⚠️ **Mutation-tested.** Deleting the `parse_safetensors_index` call fails here
    /// at *"an lfs pointer is not a safetensors file"*; deleting the
    /// `parse_adapter_config` call fails it at the DoRA case.
    #[test]
    fn a_member_that_is_not_what_it_claims_is_refused_by_loras_own_parser() {
        let (root, _) = scratch("not-an-adapter");

        let dora = root.join("dora");
        std::fs::create_dir_all(&dora).unwrap();
        std::fs::write(
            dora.join(ADAPTER_CONFIG),
            r#"{"peft_type":"LORA","r":2,"lora_alpha":4,"use_dora":true}"#,
        )
        .unwrap();
        std::fs::write(dora.join(ADAPTER_WEIGHTS), safetensors_pair()).unwrap();
        match check_adapter_dir(&dora) {
            Err(e @ AdapterRefusal::NotAnAdapter { .. }) => {
                assert!(e.sentence().contains("DoRA"), "{}", e.sentence());
            }
            other => panic!("DoRA must be refused by name, got {other:?}"),
        }

        let lfs = root.join("lfs-pointer");
        std::fs::create_dir_all(&lfs).unwrap();
        std::fs::write(lfs.join(ADAPTER_CONFIG), GOOD_CONFIG).unwrap();
        std::fs::write(
            lfs.join(ADAPTER_WEIGHTS),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:deadbeef\nsize 1234\n",
        )
        .unwrap();
        match check_adapter_dir(&lfs) {
            Err(AdapterRefusal::NotAnAdapter { path, .. }) => {
                assert!(path.ends_with(ADAPTER_WEIGHTS), "{}", path.display());
            }
            other => panic!("an lfs pointer is not a safetensors file, got {other:?}"),
        }

        // And a config that is not JSON at all.
        let junk = root.join("junk-config");
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join(ADAPTER_CONFIG), b"<html>404</html>").unwrap();
        std::fs::write(junk.join(ADAPTER_WEIGHTS), safetensors_pair()).unwrap();
        assert!(matches!(
            check_adapter_dir(&junk),
            Err(AdapterRefusal::NotAnAdapter { .. })
        ));
    }

    /// 🚨 **The whole point of the tier: a refused directory writes NOTHING.**
    ///
    /// A bad path in the sidecar is re-read and re-refused by `build_mind_graph` on
    /// **every frame** — it clears `delta`, which is the cache key that would
    /// otherwise suppress the re-read — into a terminal on a machine nobody is
    /// watching. So the refusal has to happen before the write, and it has to keep
    /// happening: this asserts the sidecar is not created by a refusal, and that an
    /// existing good selection is not destroyed by one.
    ///
    /// ⚠️ **Mutation-tested.** Reordering `select_adapter` to write first and check
    /// second fails here twice: *"a refusal must not create the sidecar"* and then
    /// *"a refusal must not disturb a selection that was already there"*.
    #[test]
    fn a_refused_directory_writes_nothing_to_the_sidecar() {
        let (root, root_abs) = scratch("no-write-on-refusal");
        let sidecar = root.join("adapter.txt");
        let bad = root.join("not-here");

        match select_adapter(&sidecar, &bad) {
            Err(AdapterSelectError::Refused(_)) => {}
            other => panic!("a missing directory must be Refused, got {other:?}"),
        }
        assert!(
            !sidecar.exists(),
            "a refusal must not create the sidecar — an unreadable path there is \
             re-refused every frame in the visual"
        );

        // Now with a good selection already standing.
        let good = root.join("good");
        std::fs::create_dir_all(&good).unwrap();
        write_adapter(&good);
        let abs = select_adapter(&sidecar, &good).expect("a complete adapter directory");
        assert_eq!(abs, root_abs.join("good"));
        let before = std::fs::read_to_string(&sidecar).unwrap();

        assert!(select_adapter(&sidecar, &bad).is_err());
        assert_eq!(
            std::fs::read_to_string(&sidecar).unwrap(),
            before,
            "a refusal must not disturb a selection that was already there"
        );
    }

    /// **Select, then clear, read back through the reader's own rule.**
    ///
    /// `adapter_selection` is `build_mind_graph`'s rule restated — trim, and treat
    /// empty as nothing selected — because `organon-world` is not a dependency of
    /// this crate and it cannot be imported. Pinning the bodies this module writes
    /// against it is what keeps `--show` from reporting a selection the lens does
    /// not have.
    ///
    /// ⚠️ **Mutation-tested.** Making `adapter_clear_body` return `"\n"` still
    /// passes (the reader trims, which is the point); making it return `"none"`
    /// fails at *"cleared must read back as nothing selected"*.
    #[test]
    fn select_then_clear_round_trips_through_the_readers_own_rule() {
        let (root, root_abs) = scratch("round-trip");
        let sidecar = root.join("adapter.txt");
        let good = root.join("good");
        std::fs::create_dir_all(&good).unwrap();
        write_adapter(&good);

        select_adapter(&sidecar, &good).unwrap();
        assert_eq!(
            read_adapter_sidecar(&sidecar).as_deref(),
            Some(root_abs.join("good").to_str().unwrap()),
            "the selection must read back as the absolute directory"
        );

        clear_adapter(&sidecar).unwrap();
        assert_eq!(
            read_adapter_sidecar(&sidecar),
            None,
            "cleared must read back as nothing selected"
        );

        // And the reader's rule itself, at the two edges `world.rs` cares about.
        assert_eq!(adapter_selection("  /a/b \r\n "), Some("/a/b"));
        assert_eq!(adapter_selection("   \n"), None);
        assert_eq!(adapter_selection(""), None);
        // A sidecar that was never written reads the same as an empty one.
        assert_eq!(read_adapter_sidecar(&root.join("never-written.txt")), None);
    }

    /// The blurb is **measured** — every field is read straight out of
    /// `adapter_config.json` and none of it is interpreted. It exists so the person
    /// selecting an adapter can tell at a glance that they picked the one they meant.
    #[test]
    fn the_blurb_reports_what_the_config_states_and_nothing_more() {
        let (rel, _) = scratch("blurb");
        write_adapter(&rel);
        let b = adapter_blurb(&rel).expect("a readable config");
        assert_eq!(b.declared_r, Some(2));
        assert_eq!(b.alpha, 4.0);
        assert!(!b.rslora);
        assert_eq!(b.target_modules, 2);
        assert_eq!(b.base.as_deref(), Some("unsloth/tiny-fixture"));
        let line = b.line();
        assert!(line.contains("rank 2"), "{line}");
        assert!(line.contains("alpha/r"), "{line}");
        assert!(line.contains("unsloth/tiny-fixture"), "{line}");

        // rsLoRA changes the denominator, and reading it the naive way understates
        // every norm by sqrt(r) with nothing erroring — so the line has to say so.
        let (rs, _) = scratch("blurb-rslora");
        std::fs::write(
            rs.join(ADAPTER_CONFIG),
            r#"{"peft_type":"LORA","r":4,"lora_alpha":8,"use_rslora":true}"#,
        )
        .unwrap();
        std::fs::write(rs.join(ADAPTER_WEIGHTS), safetensors_pair()).unwrap();
        let line = adapter_blurb(&rs).expect("a readable config").line();
        assert!(line.contains("rsLoRA"), "{line}");

        // A directory with no readable config decorates nothing rather than failing.
        assert_eq!(adapter_blurb(&rs.join("nowhere")), None);
    }
}

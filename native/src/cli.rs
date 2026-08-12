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
//! - **the console lane (#4 Tier 2)**: `console background` / `console rig`
//!   append [`ConsoleOp`] lines to [`console_cmd_path`], drained by the
//!   **console**, not by the World. A separate destination needs a separate
//!   channel — see that section's comment.

use crate::agent::{self, CliOp, SlotKind};
use crate::ipc::{self, Shared};
use crate::params::{GeneratorMode, MaterialType, SurfaceMode};
use nih_plug::prelude::Enum;

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
/// substring, against a nih-plug `Enum`'s variant names.
pub fn resolve_enum<E: Enum>(which: &str) -> Result<u32, String> {
    let vars = E::variants();
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
// Unlike `set`/`do`/`release` (fire-and-forget [`CliOp`]s through the override
// lane), `snap` and `record` need the VISUAL to do GPU work and hand a file
// path BACK. The CLI is still never an IPC writer: it appends a request line to
// `ipc::eyes_cmd_path()` (`<nonce> <verb>`), the visual drains it, acts, and
// appends a reply line to `ipc::eyes_reply_path()` (`<nonce> ok|err <text>`).
// The CLI polls the reply file for its own nonce. Both channels are append-only
// text with the same file-length/cursor drain discipline as the command
// channel. Everything here is pure (parse/format only); `bin/ctl.rs` owns the
// nonce, the I/O, and the poll loop.
// ---------------------------------------------------------------------------

/// A parsed `organon` "eyes" request (the payload of one request line, minus the
/// leading nonce).
#[derive(Debug, Clone, PartialEq)]
pub enum EyesReq {
    /// Read one frame back to a PNG at this (absolute) path.
    Snap { path: String },
    /// Start the in-app recorder; `bars` = beat-synced auto-stop length (0 = free-run).
    RecordStart { bars: u32 },
    /// Stop the in-app recorder.
    RecordStop,
}

impl EyesReq {
    /// The request line for a given nonce (single line, newline-free).
    pub fn to_line(&self, nonce: &str) -> String {
        match self {
            EyesReq::Snap { path } => format!("{nonce} snap {path}"),
            EyesReq::RecordStart { bars } => format!("{nonce} record start {bars}"),
            EyesReq::RecordStop => format!("{nonce} record stop"),
        }
    }

    /// Parse a request line → `(nonce, req)`. Rejects unknown verbs / bad bars.
    pub fn parse(line: &str) -> Option<(String, EyesReq)> {
        let line = line.trim();
        let (nonce, rest) = line.split_once(' ')?;
        let req = if let Some(p) = rest.strip_prefix("snap ") {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            EyesReq::Snap { path: p.to_string() }
        } else if let Some(b) = rest.strip_prefix("record start ") {
            EyesReq::RecordStart { bars: b.trim().parse().ok()? }
        } else if rest.trim() == "record stop" {
            EyesReq::RecordStop
        } else {
            return None;
        };
        Some((nonce.to_string(), req))
    }
}

/// Format one reply line (visual → CLI) for a nonce.
pub fn eyes_reply_line(nonce: &str, result: &Result<String, String>) -> String {
    match result {
        Ok(t) => format!("{nonce} ok {t}"),
        Err(e) => format!("{nonce} err {e}"),
    }
}

/// Scan a reply-file body for `nonce`, returning its outcome (the path text on
/// `ok`, the message on `err`) or `None` if no reply for it has landed yet.
pub fn find_eyes_reply(body: &str, nonce: &str) -> Option<Result<String, String>> {
    for line in body.lines() {
        let line = line.trim();
        let Some((n, rest)) = line.split_once(' ') else {
            continue;
        };
        if n != nonce {
            continue;
        }
        if let Some(t) = rest.strip_prefix("ok ") {
            return Some(Ok(t.to_string()));
        }
        if let Some(e) = rest.strip_prefix("err ") {
            return Some(Err(e.to_string()));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The console lane (#4 Tier 2) — the CLI's write path to the **console**.
//
// This is a third transport, not a third verb on an existing one, because it has
// a third destination (brief R3). `CliOp` lines on `ipc::cli_cmd_path()` are
// drained by the **World**, inside `World::frame_body`; the eyes channel is
// answered by the **visual**. A backdrop or a lighting rig is neither: it is
// `Shell` state, owned by `shell_main.rs`, and nothing in the World can reach it.
// Routing a console verb over `cli.txt` would put it in a queue read by a process
// that cannot act on it — green, silent, and wrong. So: its own sidecar,
// [`console_cmd_path`], drained in the console's frame path.
//
// The discipline is copied from the command channel deliberately: append-only
// UTF-8, one op per line, created on first write, reader self-detects growth by
// file length, and the CLI is never an IPC writer. Plain text, verb first — no
// JSON, matching [`CliOp`]'s simplicity.
//
// **Versioning is the verb.** There is no version token and there should not be
// one: a reader that meets a verb it does not know skips the line
// ([`parse_console_op`] returns `None`), so a newer CLI talking to an older
// console degrades to "that op did nothing" instead of to a parse error that
// poisons the rest of the drain. Adding a verb is how this format changes.
// ---------------------------------------------------------------------------

/// One console command, as it crosses the CLI→console sidecar.
///
/// The payload is an unvalidated name on purpose. `bin/ctl.rs` rejects an unknown one
/// at the clap boundary — before a line is ever written — so validation lives where the
/// error message can be good ("did you mean"), and the drain resolves what it is given.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleOp {
    /// What sits behind the glyphs: a substrate material, or a backdrop *source*
    /// (`world` / `off` / `substrate` — `shell_main.rs`'s `BackdropSource` value space).
    Background(String),
    /// The substrate's lighting rig.
    Rig(String),
}

/// `$TMPDIR/<namespace>-console.txt` — the console command channel.
///
/// Namespace-derived through [`ipc::ns_file`] like every other IPC file, which is the one
/// cross-product invariant: it is what lets a console and a plain Organon run at once
/// without either steering the other. A hard-coded `$TMPDIR` path would silently break
/// that co-existence.
///
/// It lives here rather than beside `cli_cmd_path` in `organon-core`'s `ipc.rs` because
/// `ns_file` is **public**: this lane is between the CLI and the console, and adding a
/// sidecar needs no edit to the host-free spine. If a third party ever has to build this
/// path too, moving it into `ipc.rs` is the moment — not before.
pub fn console_cmd_path() -> std::path::PathBuf {
    ipc::ns_file("console.txt")
}

/// Serialize one console op to a single sidecar line (newline-free).
pub fn console_op_to_line(op: &ConsoleOp) -> String {
    match op {
        ConsoleOp::Background(name) => format!("background {name}"),
        ConsoleOp::Rig(name) => format!("rig {name}"),
    }
}

/// Parse one sidecar line. `None` for a malformed line **or an unknown verb** — the
/// drain skips those rather than failing, which is what makes the format
/// forward-compatible (see the section comment above).
///
/// Paired with [`console_op_to_line`] so both ends of the lane — this CLI writing and
/// the console reading — speak one vocabulary from one place. That pairing is the whole
/// reason these are here and not inlined at either end.
pub fn parse_console_op(line: &str) -> Option<ConsoleOp> {
    let mut it = line.trim().split_whitespace();
    match it.next()? {
        "background" => Some(ConsoleOp::Background(it.next()?.to_string())),
        "rig" => Some(ConsoleOp::Rig(it.next()?.to_string())),
        _ => None,
    }
}

/// Append console ops to the console command channel (one line each), creating the file
/// if it is not there yet.
///
/// Fire-and-forget, exactly like [`append_ops`]: the console drains them on its next
/// frame, and ops written while no console is running are never read — deliberately not
/// replayed at its next start.
pub fn append_console_ops(ops: &[ConsoleOp]) -> std::io::Result<()> {
    use std::io::Write;
    let body: String = ops
        .iter()
        .map(|o| format!("{}\n", console_op_to_line(o)))
        .collect();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_cmd_path())?;
    f.write_all(body.as_bytes())
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
    for id in agent::ACTUATABLE_IDS {
        if !out.iter().any(|(i, _, _)| i == id) {
            out.push((id, "num", agent::id_range(id)));
        }
    }
    out
}

/// Description of a selector variant by ordinal (Layer 1 of the #452 "describe surface" —
/// the compile-enforced generator/surface/material prose, surfaced through the CLI).
fn generator_desc_at(i: usize) -> &'static str {
    agent::generator_desc(<GeneratorMode as Enum>::from_index(i))
}
fn surface_desc_at(i: usize) -> &'static str {
    agent::surface_desc(<SurfaceMode as Enum>::from_index(i))
}
fn material_desc_at(i: usize) -> &'static str {
    agent::material_desc(<MaterialType as Enum>::from_index(i))
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
        "generators": selector(GeneratorMode::variants(), &generator_desc_at),
        "surfaces": selector(SurfaceMode::variants(), &surface_desc_at),
        "materials": selector(MaterialType::variants(), &material_desc_at),
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
        GeneratorMode::variants(),
        &generator_desc_at,
    );
    section(
        "SURFACES (`organon surface <name|ordinal>`):",
        SurfaceMode::variants(),
        &surface_desc_at,
    );
    section(
        "MATERIALS (`organon material <name|ordinal>`):",
        MaterialType::variants(),
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
        let (i, name) = (i as usize, GeneratorMode::variants()[i as usize]);
        out.push_str(&format!("GENERATOR {i}  {name}\n  {}\n", generator_desc_at(i)));
    }
    if let Ok(i) = resolve_enum::<SurfaceMode>(q) {
        let (i, name) = (i as usize, SurfaceMode::variants()[i as usize]);
        out.push_str(&format!("SURFACE {i}  {name}\n  {}\n", surface_desc_at(i)));
    }
    if let Ok(i) = resolve_enum::<MaterialType>(q) {
        let (i, name) = (i as usize, MaterialType::variants()[i as usize]);
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

fn variant_name<E: Enum>(ordinal: u32) -> String {
    let vars = E::variants();
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
                GeneratorMode::variants(),
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
                SurfaceMode::variants(),
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
                MaterialType::variants(),
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
            GeneratorMode::Dna.to_index() as u32
        });
        assert_eq!(
            resolve_enum::<MaterialType>("chrome").unwrap(),
            MaterialType::Chrome.to_index() as u32
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
        assert_eq!(ops, vec![CliOp::Material(MaterialType::Glass.to_index() as u32)]);
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
            GeneratorMode::variants().len()
        );
        assert!(!j["surfaces"].as_array().unwrap().is_empty());
        assert!(j["materials"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn eyes_requests_round_trip_and_replies_match_by_nonce() {
        // Round-trip every request kind through its line form.
        for req in [
            EyesReq::Snap { path: "/tmp/a b.png".into() },
            EyesReq::RecordStart { bars: 8 },
            EyesReq::RecordStop,
        ] {
            let line = req.to_line("N1");
            assert!(!line.contains('\n'));
            assert_eq!(EyesReq::parse(&line), Some(("N1".to_string(), req)));
        }
        // Bad/empty verbs reject.
        assert!(EyesReq::parse("N1 snap ").is_none());
        assert!(EyesReq::parse("N1 record start xx").is_none());
        assert!(EyesReq::parse("N1 frobnicate").is_none());
        assert!(EyesReq::parse("noverb").is_none());

        // Reply scanning keys strictly off the nonce, ok vs err.
        let body = "N0 ok /x.png\nN1 err ffmpeg missing\nN2 ok /y.png\n";
        assert_eq!(find_eyes_reply(body, "N0"), Some(Ok("/x.png".into())));
        assert_eq!(find_eyes_reply(body, "N1"), Some(Err("ffmpeg missing".into())));
        assert_eq!(find_eyes_reply(body, "N2"), Some(Ok("/y.png".into())));
        assert_eq!(find_eyes_reply(body, "N9"), None);
        // A nonce prefix must not match a different nonce.
        assert_eq!(find_eyes_reply("N10 ok /z.png\n", "N1"), None);
        // A half-written trailing line is simply ignored until complete.
        assert_eq!(find_eyes_reply("N1 o", "N1"), None);
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
        use crate::params::{CamPath, HostFuncName, OrganicMathParams};
        use nih_plug::prelude::{BoolParam, EnumParam, FloatParam, IntParam, Param};

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
            cam_speed, cam_kick, cam_damping, mat_hue, tempo,
        );
        int!(loop_count_x, loop_count_y, loop_count_z, loop_count_q);
        boolean!(bell_physical, animate, pulse);
        choice!(
            cam_path: CamPath,
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
            (GeneratorMode::variants(), "generators.md"),
            (SurfaceMode::variants(), "surfaces.md"),
            (MaterialType::variants(), "materials.md"),
        ] {
            let md = page(file);
            for n in names {
                assert!(md.contains(&format!("## {n}\n")), "{file} is missing {n}");
            }
        }
        for (i, _) in GeneratorMode::variants().iter().enumerate() {
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
    // The console lane (#4 Tier 2)
    // -----------------------------------------------------------------------

    /// The format/parse pair is the shared vocabulary of two processes, so the round trip
    /// is the contract, not a nicety: the CLI writes with `console_op_to_line` and the
    /// console reads with `parse_console_op`, and if they ever disagree the failure is a
    /// silently ignored command rather than an error anybody sees.
    #[test]
    fn console_ops_round_trip_through_the_wire_format() {
        for op in [
            ConsoleOp::Background("graphite".into()),
            ConsoleOp::Background("paper".into()),
            ConsoleOp::Background("slate".into()),
            ConsoleOp::Background("metal".into()),
            // The three backdrop *sources* ride the same verb as the materials.
            ConsoleOp::Background("world".into()),
            ConsoleOp::Background("off".into()),
            ConsoleOp::Background("substrate".into()),
            ConsoleOp::Rig("studio".into()),
            ConsoleOp::Rig("daylight".into()),
        ] {
            let line = console_op_to_line(&op);
            assert!(!line.contains('\n'), "a line must be one line: {line:?}");
            assert_eq!(parse_console_op(&line), Some(op.clone()), "line was {line:?}");
            // The drain reads whole lines from a file; trailing whitespace is not a
            // different command.
            assert_eq!(parse_console_op(&format!("  {line}  ")), Some(op));
        }

        // The exact bytes on the wire, pinned — the console's drain is written against
        // these, and "the round trip works" would still hold if both ends changed together
        // while an already-running console spoke the old spelling.
        assert_eq!(
            console_op_to_line(&ConsoleOp::Background("slate".into())),
            "background slate"
        );
        assert_eq!(console_op_to_line(&ConsoleOp::Rig("studio".into())), "rig studio");
    }

    /// An unknown verb parses to `None` so the drain SKIPS it. That is the whole
    /// forward-compatibility story of this format: a newer CLI writing a verb an older
    /// console has never heard of must degrade to "that op did nothing", never to a parse
    /// failure that poisons the rest of the file.
    #[test]
    fn an_unknown_console_verb_is_skipped_not_fatal() {
        assert_eq!(parse_console_op("scrim 0.5"), None); // a plausible future verb
        assert_eq!(parse_console_op("frobnicate x"), None);
        assert_eq!(parse_console_op(""), None);
        assert_eq!(parse_console_op("   "), None);
        // A known verb with no argument is malformed, not a different command.
        assert_eq!(parse_console_op("background"), None);
        assert_eq!(parse_console_op("rig"), None);
        // `CliOp`'s vocabulary is a DIFFERENT lane's — a `cli.txt` line landing here (a
        // mis-wired drain) must be skipped rather than half-understood.
        assert_eq!(parse_console_op("set metallic 0.9"), None);
        assert_eq!(parse_console_op("gen 3"), None);
    }

    /// The console sidecar is namespace-derived like every other IPC file. This is the
    /// invariant that lets a console and a plain Organon run side by side; a hard-coded
    /// `$TMPDIR` name would break co-existence silently, which is precisely the failure
    /// `ns_file` exists to prevent.
    #[test]
    fn the_console_channel_is_namespaced() {
        let p = console_cmd_path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, format!("{}-console.txt", ipc::namespace()));
        // Same directory as its sibling channels — compared against one of them rather
        // than against `temp_dir()` so the assertion cannot drift from `ns_file`.
        assert_eq!(p.parent(), ipc::cli_cmd_path().parent());
        // …and it is its OWN file: routing console ops onto the World's lane is the exact
        // mistake this channel exists to avoid (brief R3).
        assert_ne!(console_cmd_path(), ipc::cli_cmd_path());
        assert_ne!(console_cmd_path(), ipc::eyes_cmd_path());
    }
}

//! The typed command service — one vocabulary (Console #5 Tier 1).
//!
//! Every action against a system flows through [`CommandService::dispatch`], and
//! **every dispatch leaves a complete [`EventKind::CommandRun`] in the session log**
//! — success, execution failure, unknown command, and argument rejection alike
//! (PRD FR-20/FR-21; `every_dispatch_leaves_a_record` pins it). A dispatch nobody
//! can audit later did not happen.
//!
//! The catalog is **data, not a trait per command** ([`CommandSpec`]), and it is
//! **seeded from outside** via [`CommandService::register_spec`] — never
//! hand-enumerated in a second place (PRD FR-47, "one vocabulary"). The engine
//! already generates its vocabulary mechanically (`agent.rs::core_catalog`, #317,
//! from the `param_block!` slot lists); the bin-side adapter that maps those
//! entries into this catalog is Tier 2+, with the real transports. T1 ships only
//! two provisional built-ins — `session.note` and `console.echo` — enough to
//! exercise dispatch end-to-end without inventing engine vocabulary here.
//!
//! ## Rejection semantics (pinned by test)
//!
//! A dispatch stopped before the target — unknown command or argument rejection —
//! logs `RunStatus::Failed` with `ended_unix_ms` set and **no target execution**.
//! Not `Denied`: session.rs reserves `Denied` for the approval model ("an action
//! the approval model stopped is not a `Failed` execution"), and T1 has no policy
//! engine — writing `Denied` for a schema rejection would fake one. For the same
//! honesty reason every record carries `approval: Automatic`: nothing was asked,
//! so nothing else may be claimed.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use serde_json::{json, Value};

use crate::session::{
    now_unix_ms, ApprovalState, Command, CommandRunRecord, EventKind, Issuer, MessageRecord,
    RunStatus, SessionEvent, SessionLog,
};

// ---------------------------------------------------------------------------
// The catalog as data
// ---------------------------------------------------------------------------

/// Whether running this command can be taken back.
///
/// 🚨 **THE RULE: a verb may run without an Enter when the console can be put back the way it
/// was.** A setting has an inverse — another value of the same verb — and a read changes
/// nothing; both are [`Reversal::Recoverable`], and getting one wrong costs a second command.
/// A verb that puts a **new element into the transcript** is [`Reversal::Permanent`]: the
/// transcript only ever grows, and there is no verb that takes an element back out of it.
///
/// ⚠️ **The test is recoverability, not severity.** Nothing in this console's vocabulary
/// formats a disk, so a severity scale would have one rung and say nothing. What a hand
/// actually needs protecting from is the edit it cannot undo.
///
/// ⚠️ **It is declared on the verb rather than decided at a draw site**, for the reason
/// `registry`'s module doc gives for everything else in the palette: a renderer that
/// classified verbs itself would be a second vocabulary, and the one that mattered would be
/// whichever ran last. `Entry` carries the same field for the view-lane verbs, which have no
/// `CommandSpec` at all — one declaration per verb, in the place that verb is declared.
///
/// ⚠️ **There is no `Default`, deliberately.** A default would let a verb added later answer
/// this question by not answering it, and the quiet answer is the one that runs. Every
/// `CommandSpec` literal in the tree has to say which it is; adding a verb is a compile error
/// until it does.
///
/// 📌 **What this is NOT: the agent's gate.** An MCP tool call never reaches this — the
/// question at that door is *"may this agent act on my behalf"*, which `start_approvals`
/// answers with a real prompt per call. That is a stronger mechanism than an Enter key, not a
/// weaker one, so `ToolEntry` deliberately does not restate this field as an annotation
/// nothing reads. The declaration is on the shared spec so both doors can read one fact when
/// the approval model wants it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversal {
    /// A read, or a setting another call can restore. Safe to run the moment the palette is
    /// certain of it.
    Recoverable,
    /// It leaves something behind that nothing can take back. Completes, then waits for Enter.
    Permanent,
}

/// What a command acts on. `as_str` yields the `CommandRunRecord::target` vocabulary
/// ("project | runtime | viewport | artifact | extension" — session.rs's doc), so the
/// log and the catalog can never spell a target two ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Runtime,
    Viewport,
    Artifact,
    Project,
    ExtensionTarget,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Runtime => "runtime",
            TargetKind::Viewport => "viewport",
            TargetKind::Artifact => "artifact",
            TargetKind::Project => "project",
            TargetKind::ExtensionTarget => "extension",
        }
    }
}

/// An argument's type + constraint. Shaped to receive `agent.rs::core_catalog`
/// 1:1 — the intended (Tier 2+) bin-side adapter maps each `CatSlot`:
///
/// - `SlotKind::Num` + `id_range(id)` → `Float { min, max }`
/// - `SlotKind::Int`                  → `Int`
/// - `SlotKind::Flag`                 → `Bool`
/// - `SlotKind::Enum`                 → `Choice(variant names)`
///
/// so a param added to a `param_block!` reaches the Console's palette with no hand edit.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgKind {
    Float { min: f64, max: f64 },
    Int,
    Bool,
    Text,
    Choice(Vec<String>),
    /// A [`ArgKind::Choice`] whose words also answer to **declared short forms** — the region
    /// vocabulary, where `tl` is `topleft` (`crate::region::REGION_ALIASES`).
    ///
    /// 🚨 **A separate variant rather than a richer `Choice`, and the reason is arithmetic.**
    /// Counted rather than estimated: `ArgKind::Choice` appears **43** times across the tree,
    /// **21** of them constructions, and `ArgSpec { … }` **50** times — so widening `Choice`'s
    /// payload or adding a field to [`ArgSpec`] would drag every vocabulary in the console into a
    /// change exactly one of them asked for. This is additive: one construction converts, the
    /// other twenty are untouched and inert (`CLAUDE.md` invariant #4), and because every reader
    /// matches `ArgKind` exhaustively the compiler names each renderer that has to learn the new
    /// arm rather than leaving one to be discovered on a running console.
    ///
    /// 🚨 **`words` is the whole DISPLAY vocabulary and `aliases` is never added to it.** The
    /// rings, `--help` and the MCP `enum` show `words`; the short forms are *accepted* and shown
    /// **beside** their word (the palette puts one in [`crate::registry::Candidate::doc`]).
    /// Listing twenty-four words where the vocabulary has twelve shapes would be the second
    /// vocabulary this registry exists to prevent, wearing the first one's clothes.
    ChoiceAliased {
        /// The canonical words, in the order they should be listed.
        words: Vec<String>,
        /// `(word, short form)`, one per aliased word. A word with no short form simply has no
        /// pair here — the two lists are not required to be the same length.
        aliases: Vec<(String, String)>,
    },
}

impl ArgKind {
    /// The canonical words of a closed value space — `None` for the kinds that have none.
    ///
    /// 📌 **The one place the two choice variants are read as the same thing.** Everything that
    /// wants "what words may go here" asks this, so a caller cannot handle `Choice` and silently
    /// skip `ChoiceAliased` — which is the failure the exhaustive matches elsewhere catch and
    /// this one, being a helper, would otherwise hide.
    pub fn choices(&self) -> Option<&[String]> {
        match self {
            ArgKind::Choice(words) | ArgKind::ChoiceAliased { words, .. } => Some(words),
            ArgKind::Float { .. } | ArgKind::Int | ArgKind::Bool | ArgKind::Text => None,
        }
    }

}

/// The one sentence saying that a closed vocabulary **also answers to short forms** — empty when
/// it does not.
///
/// 🚨 **One phrasing for every surface that has to say this.** The composer's refusal, the
/// dispatch gate's refusal, the console's own `UnknownWord` and the MCP schema's `description`
/// all reach for the same clause, and four hand-written versions of it would be four chances to
/// describe one table differently — which is the same defect as a second copy of the table,
/// arriving as prose instead of as data.
///
/// ⚠️ **Two examples, not the whole list.** Twenty-four words in a refusal is not a readable
/// sentence, and the rule (initials of the parts) is legible from one short word and one
/// compound. The examples are the **first and last** pairs, which in a largest-first vocabulary
/// is exactly that pair — and they are read out of the table rather than written here, so a
/// renamed word takes the sentence with it.
pub fn short_form_note<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let pairs: Vec<(&str, &str)> = pairs.into_iter().collect();
    let (Some(first), Some(last)) = (pairs.first(), pairs.last()) else {
        return String::new();
    };
    let examples = if first == last {
        format!("`{}` is `{}`", first.0, first.1)
    } else {
        format!("`{}` is `{}`, `{}` is `{}`", first.0, first.1, last.0, last.1)
    };
    format!(" — each has a short form: {examples}")
}

/// One declared argument. Validation guards the declared schema only: **undeclared
/// args pass through to the target** — the session.rs forward-compat stance
/// (unknown fields tolerated) applied to the call site, and what lets `console.echo`
/// take anything with an empty schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ArgSpec {
    pub name: String,
    pub kind: ArgKind,
    pub required: bool,
}

/// A catalog entry as data — deliberately **not** a trait per command, so a future
/// seeding adapter can emit hundreds of these mechanically from `core_catalog`
/// (see [`ArgKind`] for the field-level mapping) with `TargetKind::Runtime` and the
/// slot id as the name. T1's two built-ins are provisional scaffolding
/// (`session.note`, `console.echo`); they carry `TargetKind::Project` as the nearest
/// of the five kinds for session-scope actions and vanish behind real seeded
/// vocabulary in later tiers.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub name: String,
    /// One line, palette-facing.
    pub doc: String,
    pub target: TargetKind,
    pub args: Vec<ArgSpec>,
    /// Whether running it can be taken back — see [`Reversal`], which owns the rule and the
    /// reason it has no default.
    pub reversal: Reversal,
}

// ---------------------------------------------------------------------------
// Errors + outcomes
// ---------------------------------------------------------------------------

/// Structured dispatch failure. Every variant names the command, so an error is
/// self-describing when it surfaces far from the call site (palette toast, agent
/// transcript). `Clone + PartialEq` so [`MockTarget`] can script and tests can pin.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandError {
    /// No such command; `suggestion` carries the nearest catalog name when one is
    /// plausibly a typo (edit distance ≤ half the query), else `None`.
    UnknownCommand { name: String, suggestion: Option<String> },
    /// Which arg and why — "missing required argument", "out of range", "not one
    /// of […]". The dispatch never reached the target.
    InvalidArgs { command: String, arg: String, reason: String },
    /// The spec exists but no target is registered for its kind (T1's normal state
    /// for anything but the built-ins — real targets are Tier 2+).
    NoTarget { command: String, target: TargetKind },
    /// The target ran and failed.
    Execution { command: String, message: String },
    /// The session log refused the record. Loudest failure of all: an unlogged
    /// dispatch would break the every-dispatch-leaves-a-record invariant, so this
    /// supersedes whatever error it was trying to record.
    Log { command: String, message: String },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::UnknownCommand { name, suggestion } => match suggestion {
                Some(s) => write!(f, "unknown command '{name}' (did you mean '{s}'?)"),
                None => write!(f, "unknown command '{name}'"),
            },
            CommandError::InvalidArgs { command, arg, reason } => {
                write!(f, "{command}: argument '{arg}': {reason}")
            }
            CommandError::NoTarget { command, target } => {
                write!(f, "{command}: no target registered for '{}'", target.as_str())
            }
            CommandError::Execution { command, message } => write!(f, "{command}: {message}"),
            CommandError::Log { command, message } => {
                write!(f, "{command}: session log append failed: {message}")
            }
        }
    }
}

impl std::error::Error for CommandError {}

/// A successful dispatch: the target's result plus the logged event (which carries
/// the [`CommandRunRecord`] and its seq — the audit handle).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutcome {
    pub result: Value,
    pub event: SessionEvent,
}

// ---------------------------------------------------------------------------
// The execution seam
// ---------------------------------------------------------------------------

/// Where a validated command goes. One implementor per [`TargetKind`], registered
/// on the service. T1 ships only [`MockTarget`]; the real runtime target (the CLI
/// override lane / snap sidecar transport) is Tier 2+.
pub trait CommandTarget {
    fn execute(&mut self, name: &str, args: &Value) -> Result<Value, CommandError>;
}

/// Test double: records every call, plays back a scripted FIFO of results
/// (`Ok(Value::Null)` once the script runs dry). `Rc<RefCell<…>>` so the test
/// keeps a [`MockTarget::handle`] to inspect after the service consumed the box —
/// not `Send`, which a test double need not be.
#[derive(Clone, Default)]
pub struct MockTarget {
    state: Rc<RefCell<MockState>>,
}

#[derive(Default)]
struct MockState {
    calls: Vec<(String, Value)>,
    script: VecDeque<Result<Value, CommandError>>,
}

impl MockTarget {
    pub fn new() -> Self {
        Self::default()
    }

    /// A second view onto the same call record / script.
    pub fn handle(&self) -> Self {
        self.clone()
    }

    pub fn push_result(&self, result: Result<Value, CommandError>) {
        self.state.borrow_mut().script.push_back(result);
    }

    pub fn calls(&self) -> Vec<(String, Value)> {
        self.state.borrow().calls.clone()
    }
}

impl CommandTarget for MockTarget {
    fn execute(&mut self, name: &str, args: &Value) -> Result<Value, CommandError> {
        let mut s = self.state.borrow_mut();
        s.calls.push((name.to_string(), args.clone()));
        s.script.pop_front().unwrap_or(Ok(Value::Null))
    }
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

/// The in-process dispatcher: catalog + at most one target per kind + the session
/// log every dispatch is recorded in. Borrows the log rather than owning it — the
/// log outlives any one service (the app owns it; #7's bridge will write to the
/// same one).
pub struct CommandService<'log> {
    /// Sorted by name — `spec`/`register_spec` binary-search it, `list`/`suggest`
    /// read it in palette order for free.
    catalog: Vec<CommandSpec>,
    targets: Vec<(TargetKind, Box<dyn CommandTarget>)>,
    log: &'log mut SessionLog,
}

impl<'log> CommandService<'log> {
    /// A service pre-seeded with the two provisional built-ins only.
    pub fn new(log: &'log mut SessionLog) -> Self {
        let mut service = Self { catalog: Vec::new(), targets: Vec::new(), log };
        for spec in builtin_specs() {
            service.register_spec(spec);
        }
        service
    }

    /// The seeding seam (PRD FR-47): the future core_catalog adapter, extension
    /// hosts, and tests all add vocabulary through here. Replaces on name
    /// collision so re-seeding is idempotent.
    pub fn register_spec(&mut self, spec: CommandSpec) {
        match self.catalog.binary_search_by(|s| s.name.as_str().cmp(&spec.name)) {
            Ok(i) => self.catalog[i] = spec,
            Err(i) => self.catalog.insert(i, spec),
        }
    }

    /// Install (or replace) the executor for one target kind.
    pub fn register_target(&mut self, kind: TargetKind, target: Box<dyn CommandTarget>) {
        match self.targets.iter_mut().find(|(k, _)| *k == kind) {
            Some(slot) => slot.1 = target,
            None => self.targets.push((kind, target)),
        }
    }

    /// The whole catalog, sorted by name.
    pub fn list(&self) -> &[CommandSpec] {
        &self.catalog
    }

    pub fn spec(&self, name: &str) -> Option<&CommandSpec> {
        self.catalog
            .binary_search_by(|s| s.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.catalog[i])
    }

    /// Catalog names starting with `prefix`, in order — the palette's feed
    /// (Console #3 Tier 2 consumes this).
    pub fn suggest(&self, prefix: &str) -> Vec<&str> {
        self.catalog
            .iter()
            .map(|s| s.name.as_str())
            .filter(|n| n.starts_with(prefix))
            .collect()
    }

    /// Validate → execute → record. Wins and losses both land in the log; see the
    /// module doc for the rejection semantics. On `Err`, the record was still
    /// written (unless the log itself failed, which returns [`CommandError::Log`]).
    pub fn dispatch(
        &mut self,
        issuer: Issuer,
        name: &str,
        args: Value,
    ) -> Result<CommandOutcome, CommandError> {
        let started = now_unix_ms();

        let Some(spec) = self.spec(name) else {
            let err = CommandError::UnknownCommand {
                name: name.to_string(),
                suggestion: self.nearest(name),
            };
            // No spec ⇒ no target kind; the record says so rather than guessing.
            self.log_run(issuer, name, &args, "unknown", started, RunStatus::Failed)?;
            return Err(err);
        };
        let target_kind = spec.target;

        if let Err(err) = validate_args(spec, &args) {
            self.log_run(issuer, name, &args, target_kind.as_str(), started, RunStatus::Failed)?;
            return Err(err);
        }

        let result = self.execute(&issuer, target_kind, name, &args);
        let status = if result.is_ok() { RunStatus::Ok } else { RunStatus::Failed };
        let event = self.log_run(issuer, name, &args, target_kind.as_str(), started, status)?;
        result.map(|value| CommandOutcome { result: value, event })
    }

    /// Built-ins run in-process (they act on the service's own log); everything
    /// else routes to the registered target for the spec's kind.
    fn execute(
        &mut self,
        issuer: &Issuer,
        target_kind: TargetKind,
        name: &str,
        args: &Value,
    ) -> Result<Value, CommandError> {
        match name {
            "session.note" => {
                // Presence + type were validated; a missing text here is a bug.
                let text = args["text"].as_str().expect("validated").to_string();
                let event = self
                    .log
                    .append(EventKind::Message(MessageRecord { from: issuer.clone(), text }))
                    .map_err(|e| CommandError::Log {
                        command: name.to_string(),
                        message: e.to_string(),
                    })?;
                Ok(json!({ "message_seq": event.seq }))
            }
            "console.echo" => Ok(args.clone()),
            _ => match self.targets.iter_mut().find(|(k, _)| *k == target_kind) {
                Some((_, target)) => target.execute(name, args),
                None => Err(CommandError::NoTarget {
                    command: name.to_string(),
                    target: target_kind,
                }),
            },
        }
    }

    /// The one record writer — every dispatch path funnels through it, which is
    /// what makes the FR-20/FR-21 invariant structural rather than per-path.
    fn log_run(
        &mut self,
        issuer: Issuer,
        name: &str,
        args: &Value,
        target: &str,
        started_unix_ms: u64,
        status: RunStatus,
    ) -> Result<SessionEvent, CommandError> {
        let record = CommandRunRecord {
            issuer,
            command: Command { name: name.to_string(), args: args.clone() },
            target: target.to_string(),
            started_unix_ms,
            ended_unix_ms: Some(now_unix_ms()),
            status,
            evidence: Vec::new(),
            approval: ApprovalState::Automatic,
        };
        self.log
            .append(EventKind::CommandRun(record))
            .map_err(|e| CommandError::Log { command: name.to_string(), message: e.to_string() })
    }

    /// Nearest catalog name by edit distance, offered only when it is plausibly a
    /// typo (distance ≤ half the query length) — a suggestion for garbage input is
    /// noise, not help.
    fn nearest(&self, name: &str) -> Option<String> {
        self.catalog
            .iter()
            .map(|s| (levenshtein(name, &s.name), &s.name))
            .min()
            .filter(|(d, _)| d * 2 <= name.len())
            .map(|(_, n)| n.clone())
    }
}

/// The T1 provisional vocabulary — exists to exercise dispatch end-to-end, not to
/// be the vocabulary (that is seeded; see [`CommandSpec`]).
fn builtin_specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            name: "session.note".into(),
            doc: "Append a note to the session log as a Message event".into(),
            target: TargetKind::Project,
            args: vec![ArgSpec { name: "text".into(), kind: ArgKind::Text, required: true }],
            // It appends to the session log, which is append-only by design.
            reversal: Reversal::Permanent,
        },
        CommandSpec {
            name: "console.echo".into(),
            doc: "Return the arguments unchanged (dispatch-machinery probe)".into(),
            target: TargetKind::Project,
            args: Vec::new(),
            reversal: Reversal::Recoverable,
        },
    ]
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Check `args` against the declared schema: shape, required presence, per-kind
/// type + constraint. Undeclared args are tolerated (see [`ArgSpec`]).
///
/// 🚨 **A declared argument present as `null` is absent**, at both levels: a whole
/// `args` of `null` is "no arguments", and a *slot* of `null` is "this argument was
/// not given". The two readings are the same reading, and the second one was missing
/// until an optional argument existed to need it — every schema before
/// `console.camera` was required-only, so `null` in a slot could only ever have been a
/// mistake and reporting it as a type error cost nothing. It stops being free the
/// moment a verb has independent optional axes: the natural serialization of a partial
/// call is the whole slot list with `null` in the slots nobody set, and read as a type
/// error that is every partial call refused.
///
/// ⚠️ **Absence is not a bypass.** `null` only *means* anything where the schema says
/// the argument is optional — a required argument spelled `null` is still refused, and
/// now says "missing" rather than naming a type, which is the truer complaint. Nothing
/// else moves: a null in a slot the caller did declare a value for still cannot skip
/// the band, because there is no value to be out of band.
fn validate_args(spec: &CommandSpec, args: &Value) -> Result<(), CommandError> {
    let object = match args {
        // Null is "no arguments" — a caller with nothing to say shouldn't have to
        // spell `{}`.
        Value::Null => None,
        Value::Object(map) => Some(map),
        other => {
            return Err(invalid(spec, "args", format!("arguments must be a JSON object, got {}", type_name(other))))
        }
    };
    for arg in &spec.args {
        // `filter` rather than a `Some(Value::Null)` match arm, so the null case and the
        // key-absent case reach the *same* two arms below and cannot come to disagree.
        match object.and_then(|m| m.get(&arg.name)).filter(|v| !v.is_null()) {
            None if arg.required => {
                return Err(invalid(spec, &arg.name, "missing required argument".into()))
            }
            None => {}
            Some(value) => check_kind(spec, arg, value)?,
        }
    }
    Ok(())
}

fn check_kind(spec: &CommandSpec, arg: &ArgSpec, value: &Value) -> Result<(), CommandError> {
    match &arg.kind {
        ArgKind::Float { min, max } => match value.as_f64() {
            Some(v) if v >= *min && v <= *max => Ok(()),
            Some(v) => Err(invalid(spec, &arg.name, format!("out of range: {v} not in {min}..{max}"))),
            None => Err(invalid(spec, &arg.name, format!("expected a number, got {}", type_name(value)))),
        },
        // `as_i64` is None for 1.5 — a fractional value is a type error, not a
        // truncation.
        ArgKind::Int => match value.as_i64() {
            Some(_) => Ok(()),
            None => Err(invalid(spec, &arg.name, format!("expected an integer, got {}", type_name(value)))),
        },
        ArgKind::Bool => match value.as_bool() {
            Some(_) => Ok(()),
            None => Err(invalid(spec, &arg.name, format!("expected a boolean, got {}", type_name(value)))),
        },
        ArgKind::Text => match value.as_str() {
            Some(_) => Ok(()),
            None => Err(invalid(spec, &arg.name, format!("expected a string, got {}", type_name(value)))),
        },
        ArgKind::Choice(options) => match value.as_str() {
            Some(s) if options.iter().any(|o| o == s) => Ok(()),
            Some(s) => Err(invalid(
                spec,
                &arg.name,
                format!("'{s}' is not one of [{}]", options.join(", ")),
            )),
            None => Err(invalid(spec, &arg.name, format!("expected a string, got {}", type_name(value)))),
        },
        // 🚨 **This gate is what lets an abbreviation reach the MCP door at all.** The schema's
        // `enum` advertises the long words only, so a caller that read the schema is never
        // wrong — but a short form arriving here is a word the console *can* act on, and
        // refusing it would make the schema a stricter service than the one that runs, which is
        // the very thing `input_schema`'s `additionalProperties: true` note refuses to do.
        ArgKind::ChoiceAliased { words, aliases } => match value.as_str() {
            Some(s)
                if words.iter().any(|o| o == s) || aliases.iter().any(|(_, a)| a == s) =>
            {
                Ok(())
            }
            Some(s) => Err(invalid(
                spec,
                &arg.name,
                format!(
                    "'{s}' is not one of [{}]{}",
                    words.join(", "),
                    short_form_note(aliases.iter().map(|(w, a)| (w.as_str(), a.as_str()))),
                ),
            )),
            None => Err(invalid(spec, &arg.name, format!("expected a string, got {}", type_name(value)))),
        },
    }
}

fn invalid(spec: &CommandSpec, arg: &str, reason: String) -> CommandError {
    CommandError::InvalidArgs { command: spec.name.clone(), arg: arg.to_string(), reason }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Classic two-row edit distance; catalog names are short ASCII, so bytes suffice.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            curr[j + 1] = sub.min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Temp root per test, unique by pid + test name, wiped at entry — the
    /// session.rs convention (never the real store).
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("organon-console-command-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A closed-schema spec exercising every ArgKind, targeting Runtime so a
    /// MockTarget sits behind it.
    fn test_spec() -> CommandSpec {
        CommandSpec {
            name: "test.set".into(),
            doc: "test-only".into(),
            target: TargetKind::Runtime,
            args: vec![
                ArgSpec {
                    name: "gain".into(),
                    kind: ArgKind::Float { min: 0.0, max: 1.0 },
                    required: true,
                },
                ArgSpec {
                    name: "mode".into(),
                    kind: ArgKind::Choice(vec!["a".into(), "b".into()]),
                    required: false,
                },
                ArgSpec { name: "count".into(), kind: ArgKind::Int, required: false },
                ArgSpec { name: "on".into(), kind: ArgKind::Bool, required: false },
            ],
            reversal: Reversal::Recoverable,
        }
    }

    fn runs(log: &SessionLog) -> Vec<CommandRunRecord> {
        log.read_all()
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::CommandRun(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dispatch_success_logs_a_complete_ok_record() {
        let root = temp_root("ok");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);

        let args = json!({ "anything": [1, 2, 3] });
        let issuer = Issuer::Worker("claude-code".into());
        let outcome = service.dispatch(issuer.clone(), "console.echo", args.clone()).unwrap();
        // Echo returns its args verbatim — undeclared args pass validation.
        assert_eq!(outcome.result, args);

        let runs = runs(&log);
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.status, RunStatus::Ok);
        assert_eq!(run.issuer, issuer);
        assert_eq!(run.command, Command { name: "console.echo".into(), args });
        assert_eq!(run.target, "project");
        assert_eq!(run.approval, ApprovalState::Automatic);
        assert!(run.started_unix_ms > 0);
        assert!(run.ended_unix_ms.unwrap() >= run.started_unix_ms);
    }

    #[test]
    fn session_note_appends_message_then_run() {
        let root = temp_root("note");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);

        let outcome = service
            .dispatch(Issuer::PrimaryAgent, "session.note", json!({ "text": "hello" }))
            .unwrap();
        assert_eq!(outcome.result, json!({ "message_seq": 0 }));

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2, "message + run record");
        let EventKind::Message(msg) = &events[0].kind else { panic!("first is the message") };
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.from, Issuer::PrimaryAgent);
        let EventKind::CommandRun(run) = &events[1].kind else { panic!("second is the run") };
        assert_eq!(run.status, RunStatus::Ok);
    }

    #[test]
    fn target_failure_still_logs_a_failed_record() {
        let root = temp_root("target-fail");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        service.register_spec(test_spec());
        let mock = MockTarget::new();
        mock.push_result(Err(CommandError::Execution {
            command: "test.set".into(),
            message: "runtime went away".into(),
        }));
        service.register_target(TargetKind::Runtime, Box::new(mock.handle()));

        let err = service
            .dispatch(Issuer::User, "test.set", json!({ "gain": 0.5 }))
            .unwrap_err();
        assert!(matches!(err, CommandError::Execution { .. }));
        assert_eq!(mock.calls().len(), 1, "the target did run");

        let runs = runs(&log);
        assert_eq!(runs.len(), 1, "a failed execute still leaves a record");
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert_eq!(runs[0].target, "runtime");
        assert!(runs[0].ended_unix_ms.is_some());
    }

    #[test]
    fn unknown_command_suggests_and_never_reaches_a_target() {
        let root = temp_root("unknown");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        let mock = MockTarget::new();
        service.register_target(TargetKind::Runtime, Box::new(mock.handle()));

        let err = service.dispatch(Issuer::User, "session.not", json!({})).unwrap_err();
        assert_eq!(
            err,
            CommandError::UnknownCommand {
                name: "session.not".into(),
                suggestion: Some("session.note".into()),
            }
        );
        assert!(mock.calls().is_empty(), "no execution for an unknown command");

        // Garbage earns no suggestion — a far-off name is noise, not help.
        let err = service.dispatch(Issuer::User, "zzz", json!({})).unwrap_err();
        let CommandError::UnknownCommand { suggestion, .. } = err else { panic!() };
        assert_eq!(suggestion, None);

        let runs = runs(&log);
        assert_eq!(runs.len(), 2, "unknown commands are dispatches too — both logged");
        assert!(runs.iter().all(|r| r.status == RunStatus::Failed));
        assert!(runs.iter().all(|r| r.target == "unknown"));
    }

    /// The pinned rejection semantics: a validation stop logs `Failed`, never
    /// `Denied` — session.rs reserves `Denied` for the approval model, and no
    /// policy engine exists in T1 to have denied anything.
    #[test]
    fn rejected_args_log_failed_not_denied_and_skip_the_target() {
        let root = temp_root("rejected");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        service.register_spec(test_spec());
        let mock = MockTarget::new();
        service.register_target(TargetKind::Runtime, Box::new(mock.handle()));

        let cases: Vec<(Value, &str, &str)> = vec![
            (json!({}), "gain", "missing required argument"),
            (json!({ "gain": 2.0 }), "gain", "out of range: 2 not in 0..1"),
            (json!({ "gain": 0.5, "mode": "z" }), "mode", "'z' is not one of [a, b]"),
            (json!({ "gain": 0.5, "count": 1.5 }), "count", "expected an integer, got a number"),
            (json!({ "gain": 0.5, "on": "yes" }), "on", "expected a boolean, got a string"),
            (json!([1]), "args", "arguments must be a JSON object, got an array"),
        ];
        let n = cases.len();
        for (args, want_arg, want_reason) in cases {
            let err = service.dispatch(Issuer::User, "test.set", args).unwrap_err();
            let CommandError::InvalidArgs { command, arg, reason } = err else {
                panic!("expected InvalidArgs")
            };
            assert_eq!(command, "test.set");
            assert_eq!(arg, want_arg);
            assert_eq!(reason, want_reason);
        }

        assert!(mock.calls().is_empty(), "rejected args never reach the target");
        let runs = runs(&log);
        assert_eq!(runs.len(), n);
        assert!(runs.iter().all(|r| r.status == RunStatus::Failed), "Failed, not Denied");
        assert!(runs.iter().all(|r| r.approval == ApprovalState::Automatic));
    }

    /// 🚨 **An optional argument spelled `null` is absent, and this is the test that was
    /// missing when the first optional-`Float` verb shipped.**
    ///
    /// Every schema before `console.camera` had required arguments only, so the question
    /// "what does a *present* `null` mean for an argument nobody has to give?" had never been
    /// asked of this function — and the answer it gave, by accident rather than by decision,
    /// was "a type error". A caller that serializes its whole slot list with `null` in the
    /// slots it did not set (which is the natural shape for a verb whose arguments are
    /// independent axes) was therefore refused every partial call it made.
    ///
    /// The two arms of the pair are both pinned here, because the interesting property is the
    /// asymmetry: `null` is absence, and absence is only *allowed* where the schema says the
    /// argument is optional. A required argument spelled `null` is still a rejection — it just
    /// reports the honest reason ("missing") instead of a type mismatch.
    #[test]
    fn an_optional_arg_present_as_null_is_absent_and_a_required_one_is_missing() {
        let root = temp_root("null-optional");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        // The `console.camera` shape: one flag and three independent optional bands.
        service.register_spec(CommandSpec {
            name: "test.framing".into(),
            doc: "test-only".into(),
            target: TargetKind::Runtime,
            args: vec![
                ArgSpec { name: "reset".into(), kind: ArgKind::Bool, required: false },
                ArgSpec {
                    name: "yaw".into(),
                    kind: ArgKind::Float { min: -180.0, max: 180.0 },
                    required: false,
                },
                ArgSpec {
                    name: "pitch".into(),
                    kind: ArgKind::Float { min: -89.0, max: 89.0 },
                    required: false,
                },
                ArgSpec {
                    name: "distance".into(),
                    kind: ArgKind::Float { min: 5.0, max: 4000.0 },
                    required: false,
                },
            ],
            reversal: Reversal::Recoverable,
        });
        let mock = MockTarget::new();
        service.register_target(TargetKind::Runtime, Box::new(mock.handle()));

        // The flagship partial framing: one axis named, the other two present and null.
        let partial = json!({ "reset": false, "yaw": null, "pitch": null, "distance": 40.0 });
        service
            .dispatch(Issuer::User, "test.framing", partial.clone())
            .expect("a partial framing is a valid call");
        // Absent means absent for the *target* too: the null slots arrive untouched, so the
        // executor's own "which axes were named" reading is the one that decides.
        assert_eq!(mock.calls(), vec![("test.framing".to_string(), partial)]);

        // Every axis null is still valid *here* — "at least one axis" is a rule no
        // per-argument schema can state, so it belongs to the executor, not to validation.
        service
            .dispatch(Issuer::User, "test.framing", json!({ "yaw": null, "pitch": null }))
            .expect("all-null passes the schema; emptiness is the target's rule");

        // A null in an optional slot is absence, not a licence to ignore the band.
        let err = service
            .dispatch(Issuer::User, "test.framing", json!({ "yaw": null, "distance": 9000.0 }))
            .unwrap_err();
        assert_eq!(
            err,
            CommandError::InvalidArgs {
                command: "test.framing".into(),
                arg: "distance".into(),
                reason: "out of range: 9000 not in 5..4000".into(),
            }
        );

        // The other arm: a *required* argument spelled null is missing, and says so.
        service.register_spec(test_spec());
        let err = service.dispatch(Issuer::User, "test.set", json!({ "gain": null })).unwrap_err();
        assert_eq!(
            err,
            CommandError::InvalidArgs {
                command: "test.set".into(),
                arg: "gain".into(),
                reason: "missing required argument".into(),
            }
        );
    }

    #[test]
    fn missing_target_is_a_failed_run() {
        let root = temp_root("no-target");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        service.register_spec(test_spec());

        let err = service.dispatch(Issuer::User, "test.set", json!({ "gain": 0.5 })).unwrap_err();
        assert_eq!(
            err,
            CommandError::NoTarget { command: "test.set".into(), target: TargetKind::Runtime }
        );
        let runs = runs(&log);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Failed);
    }

    /// FR-20/FR-21: the invariant itself — N dispatches, N CommandRun records, no
    /// matter how each one ended.
    #[test]
    fn every_dispatch_leaves_a_record() {
        let root = temp_root("invariant");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        service.register_spec(test_spec());
        let mock = MockTarget::new();
        mock.push_result(Ok(json!("fine")));
        mock.push_result(Err(CommandError::Execution {
            command: "test.set".into(),
            message: "boom".into(),
        }));
        service.register_target(TargetKind::Runtime, Box::new(mock.handle()));

        let dispatches: Vec<(&str, Value)> = vec![
            ("test.set", json!({ "gain": 0.1 })),      // Ok
            ("test.set", json!({ "gain": 0.2 })),      // target error
            ("no.such.command", json!({})),            // unknown
            ("test.set", json!({ "gain": 9.0 })),      // rejected args
            ("session.note", json!({ "text": "n" })),  // built-in Ok
        ];
        let n = dispatches.len();
        for (name, args) in dispatches {
            let _ = service.dispatch(Issuer::Automation, name, args);
        }
        assert_eq!(runs(&log).len(), n);
    }

    #[test]
    fn catalog_list_spec_and_suggest() {
        let root = temp_root("catalog");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);

        // ⚠️ `console.` sorts BEFORE `session.` — the order flipped when `shell.echo`
        // became `console.echo` in the Console rename, which is the catalog doing exactly
        // what it says (sorted by name) rather than a regression.
        let names: Vec<&str> = service.list().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["console.echo", "session.note"], "sorted by name");
        assert_eq!(service.spec("console.echo").unwrap().target, TargetKind::Project);
        assert!(service.spec("nope").is_none());

        assert_eq!(service.suggest("session."), ["session.note"]);
        assert_eq!(service.suggest("co"), ["console.echo"]);
        assert_eq!(service.suggest(""), ["console.echo", "session.note"]);
        assert!(service.suggest("zzz").is_empty());

        // Re-seeding replaces by name — no duplicates, count unchanged.
        let mut echo = service.spec("console.echo").unwrap().clone();
        echo.doc = "replaced".into();
        service.register_spec(echo);
        assert_eq!(service.list().len(), 2);
        assert_eq!(service.spec("console.echo").unwrap().doc, "replaced");
    }

    #[test]
    fn records_round_trip_through_log_reopen() {
        let root = temp_root("roundtrip");
        {
            let mut log = SessionLog::open(&root, "s1").unwrap();
            let mut service = CommandService::new(&mut log);
            service
                .dispatch(Issuer::Worker("w1".into()), "session.note", json!({ "text": "t" }))
                .unwrap();
            let _ = service.dispatch(Issuer::User, "no.such", json!({}));
        }
        let log = SessionLog::open(&root, "s1").unwrap();
        let runs = runs(&log);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].issuer, Issuer::Worker("w1".into()));
        assert_eq!(runs[0].status, RunStatus::Ok);
        assert_eq!(runs[1].status, RunStatus::Failed);
        assert_eq!(runs[1].command.name, "no.such");
    }

    #[test]
    fn levenshtein_is_sane() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("session.not", "session.note"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}

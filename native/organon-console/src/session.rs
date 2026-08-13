//! The Shell session/event model — the shared vocabulary (Shell #4 Tier 1).
//!
//! The PRD §5 objects as serde types, a [`SessionEvent`] envelope, and an
//! append-only JSONL log with torn-tail recovery. Every other Shell tree builds on
//! these names: **#5 consumes [`CommandRunRecord`] and #7 speaks [`EventKind`]** —
//! the public field and variant names here are a contract, pinned by tests below,
//! not an implementation detail. Rename nothing without touching those issues.
//!
//! Forward compatibility is a property, not an aspiration:
//!
//! - `schema_version` lives on the **envelope** ([`SessionEvent`]), not per-struct —
//!   one number describes a whole line.
//! - Unknown fields are tolerated on deserialize (serde's default; a test pins it so
//!   nobody "tidies" a `deny_unknown_fields` in) and optional/collection fields carry
//!   `#[serde(default)]`, so a newer writer's lines still load in an older reader.
//! - [`EventKind`] is **adjacently tagged** (`{"type": "...", "data": {...}}`), so
//!   the JSONL stays human-readable: the variant name is a literal string on the
//!   line, greppable, and the payload is one nested object.
//!
//! Persistence (PRD §8.4): one directory per session under the store, one
//! `events.jsonl`, append-only. A torn final line — a crash mid-write — must not
//! poison the session: [`SessionLog::open`] quarantines the tail to
//! `events.jsonl.torn-<n>` and continues from the last good event. Artifact
//! *payloads* are Tier 2 (content-addressed store); T1 records metadata only.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version stamp written on every [`SessionEvent`] envelope. Bump when a change is
/// not covered by "unknown fields tolerated + defaults" — i.e. when a reader must
/// branch on it, never for additive growth.
pub const SCHEMA_VERSION: u32 = 1;

/// Lines written before the envelope carried a version are version 1 — the default
/// must name the version that *existed* then, never the current one.
fn schema_v1() -> u32 {
    1
}

/// Milliseconds since the Unix epoch. A pre-epoch clock yields 0 rather than a
/// panic — a wrong timestamp must never take the log down with it.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The nine first-class objects (PRD §5)
// ---------------------------------------------------------------------------

/// A configured body of work: repositories, agents, policies (PRD §5). T1 carries
/// identity + the repository list; runtimes/policies/layouts are later tiers and
/// arrive as new defaulted fields, not a v2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Repository roots (paths or URLs). A project may have none yet.
    #[serde(default)]
    pub repositories: Vec<String>,
    #[serde(default)]
    pub created_unix_ms: u64,
}

/// A durable human-and-agent work episode. Owns one event log directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub started_unix_ms: u64,
    /// `None` while the session is live — an open session is the normal state.
    #[serde(default)]
    pub ended_unix_ms: Option<u64>,
}

/// A scoped objective with ownership, state, dependencies, and evidence.
/// `state` is a free string on purpose in T1 (PRD §6.2 names seven worker states
/// and more will exist): an enum here would make every new state a schema break.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Agent id or user; `None` = unassigned.
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub state: String,
    /// Task ids this one waits on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Artifact ids backing the task's claims (PRD §4.6: evidence is part of action).
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// A connected agent's identity + declared capabilities (PRD §5 "Agent"). Named
/// `AgentInfo` because it is the *record about* an agent, not the agent runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// "primary" | "worker" | ... — free string; adapters normalize (PRD §6.2).
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub runtime_version: Option<String>,
    /// Declared capability names, granted per project policy.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Artifact **metadata** (PRD §8.5). T1 records provenance only — the
/// content-addressed payload store is Tier 2, so there is deliberately no
/// path/hash/bytes field yet to get attached to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    #[serde(default)]
    pub creator: Issuer,
    #[serde(default)]
    pub created_unix_ms: u64,
    /// The task this artifact evidences, if any.
    #[serde(default)]
    pub source_task: Option<String>,
    /// The command run that produced it, if any.
    #[serde(default)]
    pub source_command_run: Option<String>,
    #[serde(default)]
    pub mime: String,
    /// Provenance: artifact ids this one was derived from.
    #[serde(default)]
    pub inputs: Vec<String>,
}

/// A live or static view presented in Shell (viewport, diff, plot, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub id: String,
    /// "organon-viewport" | "diff" | "plot" | ... — the deck (T3) interprets it.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
}

/// A durable authority decision (PRD §6.4): the exact target and action, visible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    /// What was asked for, human-readable ("run cargo build", "publish clip").
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub target: String,
    pub state: ApprovalState,
    #[serde(default)]
    pub requested_unix_ms: u64,
    /// `None` while the request is pending.
    #[serde(default)]
    pub decided_unix_ms: Option<u64>,
}

/// A versioned, reviewable addition to Shell or Organon (PRD §5 "Extension").
/// `kind`/`state`/`scope` are free strings for the same reason as [`Task::state`]:
/// the PRD's vocabularies are open sets and the extension host is far away (§8.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionRecord {
    pub id: String,
    #[serde(default)]
    pub version: String,
    /// "panel" | "viewport lens" | "renderer" | "agent tool" | "data adapter".
    #[serde(default)]
    pub kind: String,
    /// Repository revision and/or content hash.
    #[serde(default)]
    pub source: String,
    /// "draft" | "tested" | "approved" | "active" | "disabled" | "superseded".
    #[serde(default)]
    pub state: String,
    /// Artifact ids: tests, captures, benchmarks, review notes.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// "session" | "project" | "installed".
    #[serde(default)]
    pub scope: String,
}

// ---------------------------------------------------------------------------
// CommandRun — the cross-cutting record #5 builds against
// ---------------------------------------------------------------------------

/// Who caused an action. The **enum names are the wire format** (serde external
/// tagging: `"User"`, `{"Worker":"claude-code"}`) — pinned by test, consumed by
/// Shell #5/#7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Issuer {
    #[default]
    User,
    /// Pi. Named for the role, not the persona — the record outlives any one agent.
    PrimaryAgent,
    /// A worker agent, by id.
    Worker(String),
    Automation,
}

/// A typed command with normalized arguments. `args` is open JSON on purpose: the
/// command *catalog* (#5) owns per-command argument schemas; the log records what
/// was actually sent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Command-run lifecycle. `Denied` is a terminal status of its own — an action the
/// approval model stopped is not a `Failed` execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunStatus {
    Pending,
    Running,
    Ok,
    Failed,
    Denied,
    Cancelled,
}

/// Approval disposition (PRD §6.4), shared by [`CommandRunRecord`] and [`Approval`]
/// — one vocabulary, not two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ApprovalState {
    Automatic,
    Requested,
    Granted,
    Denied,
}

/// The structured record of an action against a system (PRD §5 "Command run").
///
/// ⚠️ **Contract.** Shell #5 (command service) and #7 (agent bridge) build
/// against these exact public names; `command_run_contract_is_pinned` below fails
/// on any drift. Grow it with defaulted fields; never rename or repurpose one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandRunRecord {
    pub issuer: Issuer,
    pub command: Command,
    /// What the command acted on: project | runtime | viewport | artifact | extension.
    pub target: String,
    pub started_unix_ms: u64,
    /// `None` while running — the record is written at start and completed later.
    #[serde(default)]
    pub ended_unix_ms: Option<u64>,
    pub status: RunStatus,
    /// Artifact ids produced or cited by this run (logs, snapshots, test output).
    #[serde(default)]
    pub evidence: Vec<String>,
    pub approval: ApprovalState,
}

// ---------------------------------------------------------------------------
// The event envelope
// ---------------------------------------------------------------------------

/// Conversation content or a progress summary (PRD §6.3 `message`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageRecord {
    #[serde(default)]
    pub from: Issuer,
    #[serde(default)]
    pub text: String,
}

/// A task plan created or revised. T1 carries the visible shape only; plan
/// semantics (ownership, revision lineage) are #7's to grow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanRecord {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub steps: Vec<String>,
}

/// A requested or executed tool invocation — lighter than a [`CommandRunRecord`]:
/// tool calls are the agent's internal moves; command runs are actions against
/// systems, with approval and evidence attached.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub name: String,
    #[serde(default)]
    pub args: Value,
    /// `None` until the call completes.
    #[serde(default)]
    pub result: Option<Value>,
}

/// An artifact lifecycle moment. `action` is a free verb ("created", "updated",
/// "attached", "referenced" — PRD §6.3) with the full metadata record inline, so a
/// log line is self-describing without a lookup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactEventRecord {
    #[serde(default)]
    pub action: String,
    pub artifact: Artifact,
}

/// A surface lifecycle moment ("create", "update", "focus", "snapshot", "close").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceEventRecord {
    #[serde(default)]
    pub action: String,
    pub surface: Surface,
}

/// A task lifecycle moment ("created", "state", "assigned", "blocked", …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskEventRecord {
    #[serde(default)]
    pub action: String,
    pub task: Task,
}

/// An extension lifecycle moment ("proposed", "tested", "approved", "activated",
/// "rolled-back" — PRD §6.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionEventRecord {
    #[serde(default)]
    pub action: String,
    pub extension: ExtensionRecord,
}

/// The nine event kinds of PRD §6.3, **adjacently tagged** so a JSONL line reads
/// `"kind":{"type":"CommandRun","data":{…}}` — the variant name is a grep-able
/// literal and the payload one nested object. Unknown *fields* inside `data` are
/// tolerated; an unknown *variant name* is not (an older reader cannot invent
/// semantics for it) — that is what `schema_version` exists to gate, if ever needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventKind {
    Message(MessageRecord),
    Plan(PlanRecord),
    ToolCall(ToolCallRecord),
    ArtifactEvent(ArtifactEventRecord),
    SurfaceEvent(SurfaceEventRecord),
    TaskEvent(TaskEventRecord),
    Approval(Approval),
    CommandRun(CommandRunRecord),
    ExtensionEvent(ExtensionEventRecord),
}

/// One line of the session log. `seq` and `at_unix_ms` are assigned by
/// [`SessionLog::append`], never by callers — the log is the authority on order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub seq: u64,
    pub at_unix_ms: u64,
    /// Envelope version — one stamp per line, not per struct. Absent on a line ⇒ 1.
    #[serde(default = "schema_v1")]
    pub schema_version: u32,
    pub kind: EventKind,
}

// ---------------------------------------------------------------------------
// The append-only JSONL log
// ---------------------------------------------------------------------------

const EVENTS_FILE: &str = "events.jsonl";

/// The append-only event log for one session: `<root>/<session-id>/events.jsonl`.
///
/// Recovery (PRD §8.4 "recoverable failure states"): a crash can tear the final
/// line. On open, the longest valid prefix wins; everything after it is moved to
/// `events.jsonl.torn-<n>` (never deleted — a torn tail is still evidence) and the
/// log continues from the last good `seq`. A final line that parses but lost its
/// `\n` is a *complete* event — it is kept and the newline repaired.
pub struct SessionLog {
    dir: PathBuf,
    file: File,
    next_seq: u64,
}

impl SessionLog {
    /// The Shell store root: `dirs::data_dir()/OrganonShell` — the one-resolver
    /// rule (the #658 lesson: a hand-rolled `$HOME` put the Mind corpus in `%TEMP%`
    /// on Windows). `None` only on platforms with no data dir at all.
    pub fn store_root() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("OrganonShell"))
    }

    /// Where session directories live under the store root.
    pub fn default_sessions_root() -> Option<PathBuf> {
        Self::store_root().map(|d| d.join("sessions"))
    }

    /// Open (creating if needed) the log for `session_id` in the real store.
    /// Tests use [`SessionLog::open`] with a temp root — never this.
    pub fn open_default(session_id: &str) -> io::Result<Self> {
        let root = Self::default_sessions_root().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no platform data directory")
        })?;
        Self::open(&root, session_id)
    }

    /// Open (creating if needed) `<sessions_root>/<session_id>/events.jsonl`,
    /// running torn-tail recovery first. `next_seq` resumes after the last good
    /// event, so seq stays monotonic across process restarts.
    pub fn open(sessions_root: &Path, session_id: &str) -> io::Result<Self> {
        let dir = sessions_root.join(session_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(EVENTS_FILE);

        let mut next_seq = 0;
        if path.exists() {
            let bytes = fs::read(&path)?;
            let scan = scan_log(&bytes);
            next_seq = scan.next_seq;
            if scan.good_len < bytes.len() {
                // Quarantine, then truncate — that order, so a crash between the
                // two steps loses nothing (the tail would just be re-quarantined).
                fs::write(next_torn_path(&dir), &bytes[scan.good_len..])?;
                OpenOptions::new().write(true).open(&path)?.set_len(scan.good_len as u64)?;
            } else if scan.repair_newline {
                OpenOptions::new().append(true).open(&path)?.write_all(b"\n")?;
            }
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self { dir, file, next_seq })
    }

    /// Append one event: assigns `seq` + timestamp, writes one JSON line, flushes.
    /// Durability is to the OS, not the disk (no per-event fsync) — the torn-tail
    /// recovery above is what makes that trade safe.
    pub fn append(&mut self, kind: EventKind) -> io::Result<SessionEvent> {
        let event = SessionEvent {
            seq: self.next_seq,
            at_unix_ms: now_unix_ms(),
            schema_version: SCHEMA_VERSION,
            kind,
        };
        let mut line = serde_json::to_vec(&event).map_err(io::Error::other)?;
        line.push(b'\n');
        self.file.write_all(&line)?;
        self.file.flush()?;
        self.next_seq += 1;
        Ok(event)
    }

    /// Every event in the log, in order. Errors on a malformed line rather than
    /// skipping it — after `open`'s recovery the log is clean, so a bad line here
    /// means something else wrote to it, which must be loud.
    pub fn read_all(&self) -> io::Result<Vec<SessionEvent>> {
        let bytes = fs::read(self.events_path())?;
        bytes
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).map_err(io::Error::other))
            .collect()
    }

    /// The seq the next [`Self::append`] will assign.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// This session's directory (artifacts and sidecars land beside the log in T2).
    pub fn session_dir(&self) -> &Path {
        &self.dir
    }

    pub fn events_path(&self) -> PathBuf {
        self.dir.join(EVENTS_FILE)
    }
}

struct LogScan {
    /// Byte length of the longest valid prefix (every line parses, `\n`-terminated
    /// except possibly the last).
    good_len: usize,
    /// One past the last good event's seq; 0 for an empty log.
    next_seq: u64,
    /// The final line parsed but lost its `\n` — keep the event, restore the byte.
    repair_newline: bool,
}

/// Walk the log front-to-back; the first segment that fails to parse marks the torn
/// tail. Append-only writes mean a tear can only be terminal — anything unparseable
/// *earlier* means external interference, and quarantining from that point keeps
/// every byte while refusing to trust what follows it.
fn scan_log(bytes: &[u8]) -> LogScan {
    let mut scan = LogScan { good_len: 0, next_seq: 0, repair_newline: false };
    for segment in bytes.split_inclusive(|&b| b == b'\n') {
        let line = segment.strip_suffix(b"\n");
        match serde_json::from_slice::<SessionEvent>(line.unwrap_or(segment)) {
            Ok(event) => {
                scan.good_len += segment.len();
                scan.next_seq = event.seq + 1;
                scan.repair_newline = line.is_none();
            }
            Err(_) => break,
        }
    }
    scan
}

/// First unused `events.jsonl.torn-<n>` — earlier quarantines are evidence and are
/// never overwritten.
fn next_torn_path(dir: &Path) -> PathBuf {
    (1..)
        .map(|n| dir.join(format!("{EVENTS_FILE}.torn-{n}")))
        .find(|p| !p.exists())
        .expect("unbounded range")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Temp root per test: unique by pid + test name, wiped at entry so reruns
    /// start clean. Never the real store (std::env::temp_dir, no tempfile dep).
    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("organon-console-session-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn sample_run() -> CommandRunRecord {
        CommandRunRecord {
            issuer: Issuer::Worker("claude-code".into()),
            command: Command { name: "viewport.snapshot".into(), args: json!({ "id": "vp-1" }) },
            target: "viewport".into(),
            started_unix_ms: 1_000,
            ended_unix_ms: Some(2_000),
            status: RunStatus::Ok,
            evidence: vec!["art-1".into()],
            approval: ApprovalState::Automatic,
        }
    }

    /// One instance of every variant; the match in `kind_name` makes adding a tenth
    /// variant a compile error here until the sample (and so the round-trip) grows.
    fn sample_kinds() -> Vec<EventKind> {
        vec![
            EventKind::Message(MessageRecord { from: Issuer::PrimaryAgent, text: "hi".into() }),
            EventKind::Plan(PlanRecord { summary: "plan".into(), steps: vec!["a".into()] }),
            EventKind::ToolCall(ToolCallRecord {
                name: "read_file".into(),
                args: json!({ "path": "x" }),
                result: Some(json!("ok")),
            }),
            EventKind::ArtifactEvent(ArtifactEventRecord {
                action: "created".into(),
                artifact: Artifact {
                    id: "art-1".into(),
                    creator: Issuer::Automation,
                    created_unix_ms: 5,
                    source_task: Some("task-1".into()),
                    source_command_run: Some("run-1".into()),
                    mime: "image/png".into(),
                    inputs: vec!["art-0".into()],
                },
            }),
            EventKind::SurfaceEvent(SurfaceEventRecord {
                action: "focus".into(),
                surface: Surface { id: "s-1".into(), kind: "diff".into(), title: "d".into() },
            }),
            EventKind::TaskEvent(TaskEventRecord {
                action: "state".into(),
                task: Task {
                    id: "task-1".into(),
                    title: "t".into(),
                    owner: Some("pi".into()),
                    state: "running".into(),
                    depends_on: vec![],
                    evidence: vec![],
                },
            }),
            EventKind::Approval(Approval {
                id: "ap-1".into(),
                action: "run build".into(),
                target: "project".into(),
                state: ApprovalState::Granted,
                requested_unix_ms: 1,
                decided_unix_ms: Some(2),
            }),
            EventKind::CommandRun(sample_run()),
            EventKind::ExtensionEvent(ExtensionEventRecord {
                action: "proposed".into(),
                extension: ExtensionRecord {
                    id: "ext-1".into(),
                    version: "0.1".into(),
                    kind: "panel".into(),
                    source: "abc123".into(),
                    state: "draft".into(),
                    evidence: vec![],
                    scope: "session".into(),
                },
            }),
        ]
    }

    fn kind_name(kind: &EventKind) -> &'static str {
        match kind {
            EventKind::Message(_) => "Message",
            EventKind::Plan(_) => "Plan",
            EventKind::ToolCall(_) => "ToolCall",
            EventKind::ArtifactEvent(_) => "ArtifactEvent",
            EventKind::SurfaceEvent(_) => "SurfaceEvent",
            EventKind::TaskEvent(_) => "TaskEvent",
            EventKind::Approval(_) => "Approval",
            EventKind::CommandRun(_) => "CommandRun",
            EventKind::ExtensionEvent(_) => "ExtensionEvent",
        }
    }

    #[test]
    fn every_event_kind_round_trips() {
        let kinds = sample_kinds();
        assert_eq!(kinds.len(), 9, "sample must cover every variant");
        for kind in kinds {
            let event = SessionEvent {
                seq: 7,
                at_unix_ms: 42,
                schema_version: SCHEMA_VERSION,
                kind,
            };
            let line = serde_json::to_string(&event).unwrap();
            // The adjacent tag is a grep-able literal on the line.
            assert!(
                line.contains(&format!("\"type\":\"{}\"", kind_name(&event.kind))),
                "tag missing from {line}"
            );
            let back: SessionEvent = serde_json::from_str(&line).unwrap();
            assert_eq!(back, event);
        }
    }

    /// The drift guard for Shell #5/#7: the exact serialized field names and
    /// enum spellings of the CommandRun contract. If this fails, the fix is in
    /// those issues' court, not a rename here.
    #[test]
    fn command_run_contract_is_pinned() {
        let value = serde_json::to_value(sample_run()).unwrap();
        let mut keys: Vec<&str> =
            value.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "approval",
                "command",
                "ended_unix_ms",
                "evidence",
                "issuer",
                "started_unix_ms",
                "status",
                "target"
            ]
        );
        assert_eq!(value["issuer"], json!({ "Worker": "claude-code" }));
        assert_eq!(serde_json::to_value(Issuer::User).unwrap(), json!("User"));
        assert_eq!(serde_json::to_value(Issuer::PrimaryAgent).unwrap(), json!("PrimaryAgent"));
        assert_eq!(serde_json::to_value(Issuer::Automation).unwrap(), json!("Automation"));
        assert_eq!(value["command"], json!({ "name": "viewport.snapshot", "args": { "id": "vp-1" } }));
        assert_eq!(value["status"], json!("Ok"));
        assert_eq!(value["approval"], json!("Automatic"));
        assert_eq!(value["evidence"], json!(["art-1"]));
    }

    /// Forward compatibility is serde's default — this pins it so a well-meaning
    /// `deny_unknown_fields` can never land silently. Also covers the pre-version
    /// envelope: no `schema_version` on the line ⇒ 1.
    #[test]
    fn unknown_fields_and_missing_version_are_tolerated() {
        let line = json!({
            "seq": 0,
            "at_unix_ms": 1,
            "from_the_future": { "anything": true },
            "kind": {
                "type": "CommandRun",
                "data": {
                    "issuer": "User",
                    "command": { "name": "x", "args": null, "new_command_field": 1 },
                    "target": "project",
                    "started_unix_ms": 1,
                    "status": "Pending",
                    "approval": "Requested",
                    "new_run_field": "ignored"
                }
            }
        });
        let event: SessionEvent = serde_json::from_value(line).unwrap();
        assert_eq!(event.schema_version, 1);
        let EventKind::CommandRun(run) = &event.kind else { panic!("wrong kind") };
        assert_eq!(run.status, RunStatus::Pending);
        // Absent defaulted fields load too.
        assert_eq!(run.ended_unix_ms, None);
        assert!(run.evidence.is_empty());
    }

    #[test]
    fn seq_is_monotonic_across_reopen() {
        let root = temp_root("reopen");
        {
            let mut log = SessionLog::open(&root, "s1").unwrap();
            for kind in sample_kinds().into_iter().take(2) {
                log.append(kind).unwrap();
            }
        }
        let mut log = SessionLog::open(&root, "s1").unwrap();
        assert_eq!(log.next_seq(), 2);
        for kind in sample_kinds().into_iter().skip(2).take(2) {
            log.append(kind).unwrap();
        }
        let events = log.read_all().unwrap();
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(
            events.windows(2).all(|w| w[0].at_unix_ms <= w[1].at_unix_ms),
            "timestamps must never run backwards within a log"
        );
    }

    #[test]
    fn torn_tail_is_quarantined_not_fatal() {
        let root = temp_root("torn");
        {
            let mut log = SessionLog::open(&root, "s1").unwrap();
            for kind in sample_kinds().into_iter().take(3) {
                log.append(kind).unwrap();
            }
        }
        // A crash mid-write: half a line, no newline.
        let torn = br#"{"seq":3,"at_unix_ms":99,"kind":{"ty"#;
        let path = root.join("s1").join("events.jsonl");
        OpenOptions::new().append(true).open(&path).unwrap().write_all(torn).unwrap();

        let mut log = SessionLog::open(&root, "s1").unwrap();
        assert_eq!(log.read_all().unwrap().len(), 3, "good prefix survives");
        assert_eq!(log.next_seq(), 3);
        let quarantined = fs::read(root.join("s1").join("events.jsonl.torn-1")).unwrap();
        assert_eq!(quarantined, torn, "the torn bytes are kept, not deleted");

        // The log is whole again: appends land on a clean line.
        log.append(sample_kinds().pop().unwrap()).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[3].seq, 3);
    }

    /// A final line that parses but lost its newline is a complete event — kept,
    /// with the newline repaired so the next append does not fuse two lines.
    #[test]
    fn valid_unterminated_final_line_is_kept() {
        let root = temp_root("unterminated");
        {
            let mut log = SessionLog::open(&root, "s1").unwrap();
            log.append(sample_kinds().remove(0)).unwrap();
        }
        let path = root.join("s1").join("events.jsonl");
        let mut bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.pop(), Some(b'\n'));
        fs::write(&path, &bytes).unwrap();

        let mut log = SessionLog::open(&root, "s1").unwrap();
        assert_eq!(log.next_seq(), 1);
        log.append(sample_kinds().remove(1)).unwrap();
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert!(!root.join("s1").join("events.jsonl.torn-1").exists(), "nothing was torn");
    }

    #[test]
    fn store_root_is_under_the_platform_data_dir() {
        // dirs::data_dir() exists on every platform CI runs; the assertion is the
        // path *shape*, the one-resolver rule made visible.
        let root = SessionLog::store_root().expect("platform data dir");
        assert!(root.ends_with("OrganonShell"));
        assert!(SessionLog::default_sessions_root().unwrap().ends_with("OrganonShell/sessions"));
    }
}

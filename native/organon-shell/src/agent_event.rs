//! The decoder for an agent's structured event stream: bytes in, typed events out.
//!
//! The console's conversation view does not parse a painted character grid. It reads
//! the agent's own event stream — for Claude Code, newline-delimited JSON on a child
//! process's stdout (`--output-format stream-json`). This module is the boundary
//! between that wire format and the rest of the console: [`EventStream`] turns pipe
//! reads into whole lines, [`decode_line`] turns one line into an [`AgentEvent`], and
//! nothing downstream of here ever sees a `serde_json::Value` it did not ask for.
//!
//! It decodes. It does not reconcile, spawn, or draw. Reassembling the deltas into a
//! transcript is the consumer's policy; this module's contract is that it reports what
//! arrived, faithfully and in order.
//!
//! # Measured, not assumed
//!
//! Everything below was read off real captures from `claude.exe` 2.1.228 on this
//! machine, not from documentation. The sanitised captures live in
//! `organon-shell/fixtures/` and are what the tests at the bottom run against. The
//! surface changes weekly, so the findings are recorded here with the code that
//! depends on them:
//!
//! - **`type` is not reliably the first key.** The `result` object begins with
//!   `is_error` and carries `"type":"result"` fourth *from the end*. Any decoder that
//!   peeks at a prefix drops the single most important event in the stream. We parse
//!   the whole object before dispatching, so key order cannot matter.
//! - **`result` is a TURN terminator, not a stream terminator.** A persistent session
//!   (`--input-format stream-json`) emits one `result` per turn and keeps running;
//!   trailing `system` events also follow it in one-shot mode. Nothing here treats it
//!   as an end.
//! - **`num_turns` counts the turns of that run, and does not accumulate** across a
//!   persistent session — it was `1` on both results of a verified two-turn stream.
//!   `total_cost_usd` and the (unmodelled) `modelUsage` block *are* cumulative, while
//!   the sibling `usage` block is per-turn. Do not add costs across results.
//! - **`system`/`init` recurs mid-stream.** A second init arrived before turn two of
//!   the same session, with a *different* field count. It is a re-announcement, not a
//!   new session; the `session_id` is what identifies the session.
//! - **An `assistant` line carries one content block, not the whole message.** Three
//!   consecutive lines shared one `message.id`, holding text, then the first
//!   `tool_use`, then the second. Their `usage.output_tokens` is a placeholder (`1`);
//!   the real count arrives in the `message_delta` stream event.
//! - **Order is not turn-sequential.** A `user` tool_result was observed *between* two
//!   `input_json_delta`s of a later content block.
//! - **Key casing is mixed.** `session_id` and `claude_code_version` are snake_case;
//!   `permissionMode`, `isReplay`, `resetsAt` and `rateLimitType` are camelCase, in
//!   the same objects. Every rename here is a measurement.
//! - **Non-JSON lines share the pipe.** The first line of one capture is a plain-text
//!   warning about stdin. It is reported as a [`DecodeError::NotJson`] carrying the
//!   text — never swallowed, never fatal.
//!
//! # Degrading, by construction
//!
//! Claude Code's own guidance is to feature-detect rather than version-compare, and
//! the captures already contain events documented nowhere (`rate_limit_event`,
//! `system`/`post_turn_summary`, `system`/`task_summary`). So an unknown *type*, an
//! unknown *subtype*, an unknown *content block*, an unknown *delta* and an unknown
//! *stream event* each have their own explicit variant that preserves the original
//! JSON: [`EventKind::Unknown`], [`Notice`], [`ContentBlock::Unknown`],
//! [`Delta::Unknown`], [`StreamEvent::Unknown`]. Nothing is dropped with a silent
//! wildcard, and a schema change is a rendering gap rather than a decode failure.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::fmt;

// ---------------------------------------------------------------------------
// The event
// ---------------------------------------------------------------------------

/// One decoded line of the agent's event stream.
///
/// The envelope fields are the ones that appear across event types; everything
/// specific to a type lives in [`AgentEvent::kind`].
#[derive(Debug, Clone, PartialEq)]
pub struct AgentEvent {
    /// The session this line belongs to. Stable for the life of a persistent
    /// session, across every turn in it.
    pub session_id: Option<String>,
    /// The CLI's own id for this line. Referenced by `post_turn_summary`'s
    /// `summarizes_uuid`, so it is the handle for correlating a summary to what it
    /// summarises.
    pub uuid: Option<String>,
    /// ISO-8601, present on `assistant` and `user` lines only.
    pub timestamp: Option<String>,
    /// Which agent is speaking — the main one, or a subagent running inside a tool
    /// call. Decoded from `parent_tool_use_id`, whose `null` is meaningful.
    pub scope: AgentScope,
    /// What the line actually says.
    pub kind: EventKind,
}

impl AgentEvent {
    /// True when this line came from a subagent rather than the main conversation.
    pub fn is_from_subagent(&self) -> bool {
        matches!(self.scope, AgentScope::Subagent { .. })
    }

    /// The tool call a subagent is running inside, if any.
    pub fn subagent_tool_use_id(&self) -> Option<&str> {
        match &self.scope {
            AgentScope::Subagent { tool_use_id } => Some(tool_use_id.as_str()),
            AgentScope::Main => None,
        }
    }
}

/// Whose voice a line is in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AgentScope {
    /// The main conversation.
    #[default]
    Main,
    /// A subagent, running inside the named tool call.
    Subagent { tool_use_id: String },
}

/// What a line says. One variant per top-level `type`, plus the escape hatch.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    /// `system`/`init` — the session announcing its cwd, model, tools and version.
    /// Recurs mid-session; see the module docs.
    SessionStarted(SessionStart),
    /// Any other `system` line. The subtype is preserved verbatim and the whole
    /// object is kept, because these are undocumented and change often.
    Notice(Notice),
    /// `assistant` — one settled content block of an assistant message.
    Assistant(AssistantTurn),
    /// `user` — tool results, genuine human input, or (in principle) both.
    User(UserTurn),
    /// `stream_event` — the incremental view, unwrapped from its envelope.
    Stream(StreamEvent),
    /// `result` — the end of a *turn*, with its usage and cost.
    Finished(TurnResult),
    /// `rate_limit_event` — quota state, undocumented but present in every capture.
    RateLimit(RateLimit),
    /// A `type` this build does not know. Preserved whole so a newer CLI cannot
    /// silently lose information.
    Unknown { event_type: String, body: Value },
}

// ---------------------------------------------------------------------------
// system / init
// ---------------------------------------------------------------------------

/// `system`/`init`: what the CLI says about itself at the start of a turn.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct SessionStart {
    /// The working directory the agent operates in.
    #[serde(default)]
    pub cwd: String,
    /// e.g. `claude-opus-5[1m]` — the suffix is part of the string.
    #[serde(default)]
    pub model: String,
    /// Every tool name available to the agent, MCP tools included.
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
    /// camelCase on the wire, in an otherwise snake_case object.
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: String,
    /// The CLI's version. Reported, never compared against — feature-detect instead.
    #[serde(default, rename = "claude_code_version")]
    pub cli_version: String,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub output_style: Option<String>,
}

/// One MCP server and whether it came up.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct McpServer {
    #[serde(default)]
    pub name: String,
    /// `connected`, `pending`, `failed`, … reported as given.
    #[serde(default)]
    pub status: String,
}

// ---------------------------------------------------------------------------
// system / everything else
// ---------------------------------------------------------------------------

/// A `system` line that is not `init`.
///
/// These are the volatile part of the surface — `status`, `task_summary`,
/// `post_turn_summary` are all real and all undocumented — so the body is kept whole
/// and read through named accessors rather than pinned into a struct that would need
/// changing every time the CLI grows a field.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    /// The `subtype`, verbatim. Empty when the line carried none.
    pub subtype: String,
    /// The entire original object, `type` and envelope included.
    pub body: Value,
}

impl Notice {
    /// Any string field of the original object.
    pub fn field(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str)
    }

    /// `system`/`status`: `requesting`, and friends.
    pub fn status(&self) -> Option<&str> {
        self.field("status")
    }

    /// `system`/`task_summary`: a one-line gloss of what the agent is doing. Null on
    /// the trailing summary that closes a run, so this is `None` there.
    pub fn detail(&self) -> Option<&str> {
        self.field("detail")
    }

    /// `system`/`post_turn_summary`: e.g. `review_ready`.
    pub fn status_category(&self) -> Option<&str> {
        self.field("status_category")
    }

    /// `system`/`post_turn_summary`: a sentence describing the finished turn.
    pub fn status_detail(&self) -> Option<&str> {
        self.field("status_detail")
    }

    /// `system`/`post_turn_summary`: what the human is being asked for. Observed
    /// empty on turns that need nothing.
    pub fn needs_action(&self) -> Option<&str> {
        self.field("needs_action").filter(|s| !s.is_empty())
    }

    /// `system`/`post_turn_summary`: the [`AgentEvent::uuid`] of the message this
    /// summarises.
    pub fn summarizes_uuid(&self) -> Option<&str> {
        self.field("summarizes_uuid")
    }
}

// ---------------------------------------------------------------------------
// assistant
// ---------------------------------------------------------------------------

/// An `assistant` line: one settled content block, with the message it belongs to.
///
/// Several arrive per assistant message, sharing a [`message_id`](Self::message_id).
/// They are the complete counterpart of the deltas, not a replacement for them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssistantTurn {
    /// The API message id. Shared by every line of one assistant message, which is
    /// how the consumer groups them.
    pub message_id: Option<String>,
    pub model: Option<String>,
    /// `tool_use`, `end_turn`, … Observed `null` on `assistant` lines; the settled
    /// value arrives on the `message_delta` stream event.
    pub stop_reason: Option<String>,
    /// Token counts. `output_tokens` is a placeholder here — see the module docs.
    pub usage: Option<Usage>,
    /// The blocks on this line. One, in every capture.
    pub content: Vec<ContentBlock>,
    pub request_id: Option<String>,
}

impl AssistantTurn {
    /// The text of every text block on this line, concatenated.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for block in &self.content {
            if let ContentBlock::Text(text) = block {
                out.push_str(text);
            }
        }
        out
    }

    /// Every tool call on this line.
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCall> {
        self.content.iter().filter_map(|block| match block {
            ContentBlock::ToolUse(call) => Some(call),
            _ => None,
        })
    }
}

/// A block of assistant content.
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(String),
    /// Extended thinking. `signature` is the opaque attestation the API returns.
    Thinking {
        text: String,
        signature: Option<String>,
    },
    ToolUse(ToolCall),
    /// A block kind this build does not know — `server_tool_use`,
    /// `web_search_tool_result`, whatever ships next. Preserved whole.
    Unknown { block_type: String, body: Value },
}

/// The agent asking for a tool to be run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolCall {
    /// Correlates with [`ToolOutcome::tool_use_id`].
    pub id: String,
    /// `Read`, `Bash`, `mcp__server__tool`, …
    pub name: String,
    /// The complete arguments object. Empty (`{}`) on the `content_block_start` that
    /// opens a streaming call — the arguments arrive as [`Delta::ToolInputJson`]
    /// fragments and are complete only on the settled `assistant` line.
    pub input: Value,
}

// ---------------------------------------------------------------------------
// user
// ---------------------------------------------------------------------------

/// A `user` line.
///
/// The type is overloaded: the same `type` carries the results of tool calls and
/// genuine human input. Rather than make the consumer re-derive that from block
/// kinds, the blocks are split on the way in and [`UserTurn::voice`] names the case.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserTurn {
    /// `tool_result` blocks, correlated back to their calls.
    pub tool_results: Vec<ToolOutcome>,
    /// Human text. Also carries the whole content when the CLI sends the bare-string
    /// form of a user message.
    pub text: Vec<String>,
    /// Blocks that are neither — images, documents, future kinds. Preserved whole.
    pub other: Vec<Value>,
    /// True when this is the CLI echoing input back under
    /// `--replay-user-messages` (`isReplay` on the wire), rather than an event the
    /// conversation generated. The echo is how a human turn appears in stream order.
    pub replay: bool,
    /// The undocumented `tool_use_result` sibling of `message`, carrying structured
    /// detail the text content does not — for `Read`, the file path, content and line
    /// counts. Kept verbatim because its shape varies per tool.
    pub tool_use_result: Option<Value>,
}

impl UserTurn {
    /// What this line actually is.
    pub fn voice(&self) -> UserVoice {
        match (self.tool_results.is_empty(), self.text.is_empty()) {
            (false, true) => UserVoice::ToolResponse,
            (true, false) => UserVoice::Human,
            (false, false) => UserVoice::Mixed,
            (true, true) => UserVoice::Empty,
        }
    }

    /// The human text of this line, concatenated. Empty for a tool response.
    pub fn human_text(&self) -> String {
        self.text.join("")
    }
}

/// The discrimination trap 3 exists to close: which kind of `user` line this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserVoice {
    /// Tool results only — the conversation talking to itself.
    ToolResponse,
    /// Human text only.
    Human,
    /// Both, in one line. Not observed in any capture; modelled so it cannot be lost.
    Mixed,
    /// Neither — an image-only or otherwise unrecognised body.
    Empty,
}

/// The result of a tool call, coming back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolOutcome {
    /// Correlates with [`ToolCall::id`].
    pub tool_use_id: String,
    /// A string in every capture, but the API also permits an array of blocks, so
    /// the raw value is kept. Use [`ToolOutcome::text`] for the common case.
    pub content: Value,
    /// Absent on success in the captures; defaulted to `false`.
    pub is_error: bool,
}

impl ToolOutcome {
    /// The result as text: the string form directly, or the text blocks of the array
    /// form joined.
    pub fn text(&self) -> String {
        match &self.content {
            Value::String(text) => text.clone(),
            Value::Array(items) => {
                let mut out = String::new();
                for item in items {
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
                out
            }
            _ => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// stream_event
// ---------------------------------------------------------------------------

/// The incremental view, unwrapped from its `stream_event` envelope.
///
/// These are the raw API streaming events. They arrive interleaved with the settled
/// `assistant` and `user` lines covering the same content — both are reported, and
/// choosing between them is the consumer's policy.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A new assistant message opens.
    MessageStart {
        message_id: Option<String>,
        model: Option<String>,
        usage: Option<Usage>,
    },
    /// A content block opens at `index`. For a tool call the block's input is empty
    /// here; the fragments follow.
    BlockStart { index: usize, block: ContentBlock },
    /// A fragment for the block at `index`.
    BlockDelta { index: usize, delta: Delta },
    /// The block at `index` is complete.
    BlockStop { index: usize },
    /// The message settles: the real stop reason and the real output token count.
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<Usage>,
    },
    /// The message is complete.
    MessageStop,
    /// An inner event kind this build does not know, or one whose `index` was
    /// missing so its fragments could not be attributed. Preserved whole rather than
    /// guessed at — a delta applied to the wrong block is worse than one not applied.
    Unknown { event_type: String, body: Value },
}

/// A fragment of a content block.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta {
    /// `text_delta` — assistant prose, one piece at a time.
    Text(String),
    /// `input_json_delta` — a fragment of a tool call's arguments. These are pieces
    /// of a JSON *string*, not JSON values; concatenate every fragment for one block
    /// before parsing, or wait for the settled `assistant` line.
    ToolInputJson(String),
    /// `thinking_delta`.
    Thinking(String),
    /// `signature_delta` — the attestation for a thinking block.
    Signature(String),
    /// A delta kind this build does not know. Preserved whole.
    Unknown { delta_type: String, body: Value },
}

// ---------------------------------------------------------------------------
// result
// ---------------------------------------------------------------------------

/// The `result` line: the end of a turn.
///
/// Not the end of the stream. In a persistent session one of these arrives per turn
/// and more turns follow; in one-shot mode `system` lines follow it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TurnResult {
    /// The wire's first key — which is why this type is decoded whole rather than
    /// sniffed.
    pub is_error: bool,
    /// `success`, `error_during_execution`, `error_max_turns`, …
    pub subtype: String,
    /// The agent's final text for the turn. Absent on error subtypes.
    pub text: Option<String>,
    /// Turns in *this run*. Does not accumulate across a persistent session.
    pub num_turns: u64,
    pub duration_ms: Option<u64>,
    pub duration_api_ms: Option<u64>,
    /// Cumulative across the session, unlike the per-turn [`usage`](Self::usage).
    pub total_cost_usd: Option<f64>,
    /// This turn's tokens.
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
    /// `completed`, `error`, …
    pub terminal_reason: Option<String>,
    /// Non-null only when the API itself failed; an HTTP status in the one capture
    /// that carries it.
    pub api_error_status: Option<Value>,
    /// Tools the user refused. Shape is undocumented, so kept verbatim.
    pub permission_denials: Vec<Value>,
}

/// Token counts, as they appear on messages, deltas and results alike.
///
/// Every field defaults, because which of them a given object carries varies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

// ---------------------------------------------------------------------------
// rate_limit_event
// ---------------------------------------------------------------------------

/// `rate_limit_event` — quota state. Undocumented; present as the first line of
/// every capture taken on this machine.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RateLimit {
    /// `allowed`, and whatever else the service says.
    pub status: String,
    /// Unix seconds at which the window resets. `resetsAt` on the wire.
    pub resets_at: Option<i64>,
    /// `five_hour`, … `rateLimitType` on the wire.
    pub limit_type: Option<String>,
    /// `rejected`, … `overageStatus` on the wire.
    pub overage_status: Option<String>,
    /// `isUsingOverage` on the wire.
    pub using_overage: bool,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a line could not become an event.
///
/// Deliberately narrow. An unknown *event* is never an error — it decodes to an
/// [`EventKind::Unknown`]. These three are the cases where there is no event at all.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// The line was not JSON. The CLI writes plain-text warnings to the same pipe
    /// (one capture opens with a stdin warning), so this is expected traffic — log it
    /// and carry on. The text is preserved.
    NotJson { line: String, message: String },
    /// Valid JSON, but not an object, so it cannot be an event.
    NotAnObject { line: String },
    /// A JSON object with no `type` key at all.
    NoType { body: Value },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::NotJson { line, message } => {
                write!(f, "not JSON ({message}): {}", Preview(line))
            }
            DecodeError::NotAnObject { line } => {
                write!(f, "JSON but not an object: {}", Preview(line))
            }
            DecodeError::NoType { body } => {
                write!(f, "object with no `type` key: {}", Preview(&body.to_string()))
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Truncates a line for an error message. A `tool_result` can carry a whole file, and
/// an error string is not the place for it.
struct Preview<'a>(&'a str);

impl fmt::Display for Preview<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const LIMIT: usize = 120;
        if self.0.len() <= LIMIT {
            return f.write_str(self.0);
        }
        let mut end = LIMIT;
        while end > 0 && !self.0.is_char_boundary(end) {
            end -= 1;
        }
        write!(f, "{}… ({} bytes)", &self.0[..end], self.0.len())
    }
}

// ---------------------------------------------------------------------------
// Buffering: whole lines, or nothing
// ---------------------------------------------------------------------------

/// Turns pipe reads into whole lines, then into events.
///
/// A read from a child's stdout splits wherever the OS felt like splitting it, and a
/// `tool_result` line can carry an entire file, so a split mid-line is the normal
/// case rather than an edge one. This type owns that buffering explicitly: it emits
/// an event only for a line it has seen the end of, so there is no way to hand it a
/// fragment and get a wrong answer back.
///
/// Bytes, not `&str`, on purpose — a chunk boundary can fall inside a multi-byte
/// character, and decoding per chunk would corrupt it. Splitting on `\n` first is
/// safe because `\n` cannot occur inside a UTF-8 multi-byte sequence.
///
/// ```
/// use organon_shell::agent_event::EventStream;
/// let mut stream = EventStream::new();
/// assert!(stream.push(br#"{"type":"system","subty"#).is_empty());
/// let events = stream.push(br#"pe":"status","status":"requesting"}"#.as_ref());
/// assert!(events.is_empty(), "no newline yet, so no event yet");
/// let events = stream.push(b"\n");
/// assert_eq!(events.len(), 1);
/// ```
#[derive(Debug, Default, Clone)]
pub struct EventStream {
    buf: Vec<u8>,
}

impl EventStream {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Feeds a chunk of bytes, returning one outcome per *complete* line it finished.
    ///
    /// Blank lines are dropped; a line that is not JSON comes back as an `Err` rather
    /// than vanishing.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Result<AgentEvent, DecodeError>> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(offset) = self.buf[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset;
            if let Some(outcome) = decode_bytes(&self.buf[start..end]) {
                out.push(outcome);
            }
            start = end + 1;
        }
        if start > 0 {
            self.buf.drain(..start);
        }
        out
    }

    /// Decodes whatever is left when the pipe closes without a final newline.
    ///
    /// Call this once at EOF and never before: until the process ends, a buffer with
    /// no newline is an unfinished line, not a short one.
    pub fn flush(&mut self) -> Option<Result<AgentEvent, DecodeError>> {
        if self.buf.is_empty() {
            return None;
        }
        let line = std::mem::take(&mut self.buf);
        decode_bytes(&line)
    }

    /// How many bytes are held in the partial line. Diagnostics only.
    pub fn pending_bytes(&self) -> usize {
        self.buf.len()
    }
}

fn decode_bytes(line: &[u8]) -> Option<Result<AgentEvent, DecodeError>> {
    let text = String::from_utf8_lossy(line);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(decode_line(trimmed))
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decodes one complete NDJSON line.
///
/// The line must be whole — use [`EventStream`] if you are reading from a pipe.
pub fn decode_line(line: &str) -> Result<AgentEvent, DecodeError> {
    let trimmed = line.trim();
    let value: Value = serde_json::from_str(trimmed).map_err(|error| DecodeError::NotJson {
        line: trimmed.to_string(),
        message: error.to_string(),
    })?;
    decode_value(value).map_err(|error| match error {
        // Re-attach the original text, which `decode_value` never saw.
        DecodeError::NotAnObject { .. } => DecodeError::NotAnObject {
            line: trimmed.to_string(),
        },
        other => other,
    })
}

/// Decodes an already-parsed JSON value. The seam for callers who have the object
/// from somewhere other than a pipe.
pub fn decode_value(value: Value) -> Result<AgentEvent, DecodeError> {
    let map = match value {
        Value::Object(map) => map,
        other => {
            return Err(DecodeError::NotAnObject {
                line: other.to_string(),
            })
        }
    };

    let event_type = match string_at(&map, "type") {
        Some(event_type) => event_type,
        None => return Err(DecodeError::NoType { body: Value::Object(map) }),
    };

    let session_id = string_at(&map, "session_id");
    let uuid = string_at(&map, "uuid");
    let timestamp = string_at(&map, "timestamp");
    let scope = match map.get("parent_tool_use_id") {
        Some(Value::String(tool_use_id)) => AgentScope::Subagent {
            tool_use_id: tool_use_id.clone(),
        },
        _ => AgentScope::Main,
    };

    let kind = match event_type.as_str() {
        "system" => decode_system(map),
        "assistant" => decode_assistant(&map),
        "user" => decode_user(&map),
        "stream_event" => decode_stream(&map),
        "result" => decode_result(&map),
        "rate_limit_event" => decode_rate_limit(&map),
        _ => EventKind::Unknown {
            event_type,
            body: Value::Object(map),
        },
    };

    Ok(AgentEvent {
        session_id,
        uuid,
        timestamp,
        scope,
        kind,
    })
}

fn decode_system(map: Map<String, Value>) -> EventKind {
    let subtype = string_at(&map, "subtype").unwrap_or_default();
    if subtype == "init" {
        // Cloned only for `init`, which is rare; if the shape ever drifts far enough
        // that this fails, the line still survives as a Notice rather than vanishing.
        if let Ok(start) = serde_json::from_value::<SessionStart>(Value::Object(map.clone())) {
            return EventKind::SessionStarted(start);
        }
    }
    EventKind::Notice(Notice {
        subtype,
        body: Value::Object(map),
    })
}

fn decode_assistant(map: &Map<String, Value>) -> EventKind {
    let message = map.get("message");
    EventKind::Assistant(AssistantTurn {
        message_id: message.and_then(|m| string_of(m.get("id"))),
        model: message.and_then(|m| string_of(m.get("model"))),
        stop_reason: message.and_then(|m| string_of(m.get("stop_reason"))),
        usage: message.and_then(|m| usage_of(m.get("usage"))),
        content: content_blocks(message.and_then(|m| m.get("content"))),
        request_id: string_at(map, "request_id"),
    })
}

fn decode_user(map: &Map<String, Value>) -> EventKind {
    let mut turn = UserTurn {
        replay: map
            .get("isReplay")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        tool_use_result: map.get("tool_use_result").cloned(),
        ..UserTurn::default()
    };

    match map.get("message").and_then(|m| m.get("content")) {
        // The bare-string form the stream-json *input* schema uses.
        Some(Value::String(text)) => turn.text.push(text.clone()),
        Some(Value::Array(items)) => {
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("tool_result") => turn.tool_results.push(ToolOutcome {
                        tool_use_id: string_of(item.get("tool_use_id")).unwrap_or_default(),
                        content: item.get("content").cloned().unwrap_or(Value::Null),
                        is_error: item
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    }),
                    Some("text") => {
                        turn.text.push(string_of(item.get("text")).unwrap_or_default())
                    }
                    _ => turn.other.push(item.clone()),
                }
            }
        }
        _ => {}
    }

    EventKind::User(turn)
}

fn decode_stream(map: &Map<String, Value>) -> EventKind {
    let event = match map.get("event") {
        Some(Value::Object(event)) => event,
        _ => {
            return EventKind::Stream(StreamEvent::Unknown {
                event_type: String::new(),
                body: Value::Object(map.clone()),
            })
        }
    };
    let event_type = string_at(event, "type").unwrap_or_default();
    let unknown = || StreamEvent::Unknown {
        event_type: event_type.clone(),
        body: Value::Object(event.clone()),
    };
    // An event that names a block must say which one. Guessing an index would
    // misattribute the fragment, so a missing one degrades to Unknown instead.
    let index = event.get("index").and_then(Value::as_u64).map(|i| i as usize);

    let stream = match event_type.as_str() {
        "message_start" => {
            let message = event.get("message");
            StreamEvent::MessageStart {
                message_id: message.and_then(|m| string_of(m.get("id"))),
                model: message.and_then(|m| string_of(m.get("model"))),
                usage: message.and_then(|m| usage_of(m.get("usage"))),
            }
        }
        "content_block_start" => match index {
            Some(index) => StreamEvent::BlockStart {
                index,
                block: content_block(event.get("content_block").unwrap_or(&Value::Null)),
            },
            None => unknown(),
        },
        "content_block_delta" => match index {
            Some(index) => StreamEvent::BlockDelta {
                index,
                delta: delta_of(event.get("delta").unwrap_or(&Value::Null)),
            },
            None => unknown(),
        },
        "content_block_stop" => match index {
            Some(index) => StreamEvent::BlockStop { index },
            None => unknown(),
        },
        "message_delta" => StreamEvent::MessageDelta {
            stop_reason: event.get("delta").and_then(|d| string_of(d.get("stop_reason"))),
            usage: usage_of(event.get("usage")),
        },
        "message_stop" => StreamEvent::MessageStop,
        _ => unknown(),
    };
    EventKind::Stream(stream)
}

fn decode_result(map: &Map<String, Value>) -> EventKind {
    EventKind::Finished(TurnResult {
        is_error: map.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        subtype: string_at(map, "subtype").unwrap_or_default(),
        text: string_at(map, "result"),
        num_turns: map.get("num_turns").and_then(Value::as_u64).unwrap_or(0),
        duration_ms: map.get("duration_ms").and_then(Value::as_u64),
        duration_api_ms: map.get("duration_api_ms").and_then(Value::as_u64),
        total_cost_usd: map.get("total_cost_usd").and_then(Value::as_f64),
        usage: usage_of(map.get("usage")),
        stop_reason: string_at(map, "stop_reason"),
        terminal_reason: string_at(map, "terminal_reason"),
        api_error_status: map
            .get("api_error_status")
            .filter(|value| !value.is_null())
            .cloned(),
        permission_denials: map
            .get("permission_denials")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    })
}

fn decode_rate_limit(map: &Map<String, Value>) -> EventKind {
    let info = map.get("rate_limit_info");
    EventKind::RateLimit(RateLimit {
        status: info
            .and_then(|i| string_of(i.get("status")))
            .unwrap_or_default(),
        resets_at: info.and_then(|i| i.get("resetsAt")).and_then(Value::as_i64),
        limit_type: info.and_then(|i| string_of(i.get("rateLimitType"))),
        overage_status: info.and_then(|i| string_of(i.get("overageStatus"))),
        using_overage: info
            .and_then(|i| i.get("isUsingOverage"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn content_blocks(content: Option<&Value>) -> Vec<ContentBlock> {
    match content {
        Some(Value::Array(items)) => items.iter().map(content_block).collect(),
        Some(Value::String(text)) => vec![ContentBlock::Text(text.clone())],
        _ => Vec::new(),
    }
}

fn content_block(block: &Value) -> ContentBlock {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => ContentBlock::Text(string_of(block.get("text")).unwrap_or_default()),
        Some("thinking") => ContentBlock::Thinking {
            text: string_of(block.get("thinking")).unwrap_or_default(),
            signature: string_of(block.get("signature")),
        },
        Some("tool_use") => ContentBlock::ToolUse(ToolCall {
            id: string_of(block.get("id")).unwrap_or_default(),
            name: string_of(block.get("name")).unwrap_or_default(),
            input: block.get("input").cloned().unwrap_or(Value::Null),
        }),
        other => ContentBlock::Unknown {
            block_type: other.unwrap_or_default().to_string(),
            body: block.clone(),
        },
    }
}

fn delta_of(delta: &Value) -> Delta {
    match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => Delta::Text(string_of(delta.get("text")).unwrap_or_default()),
        Some("input_json_delta") => {
            Delta::ToolInputJson(string_of(delta.get("partial_json")).unwrap_or_default())
        }
        Some("thinking_delta") => {
            Delta::Thinking(string_of(delta.get("thinking")).unwrap_or_default())
        }
        Some("signature_delta") => {
            Delta::Signature(string_of(delta.get("signature")).unwrap_or_default())
        }
        other => Delta::Unknown {
            delta_type: other.unwrap_or_default().to_string(),
            body: delta.clone(),
        },
    }
}

fn string_at(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_of(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn usage_of(value: Option<&Value>) -> Option<Usage> {
    value
        .cloned()
        .and_then(|value| serde_json::from_value::<Usage>(value).ok())
}

/// Decodes a whole buffer of NDJSON at once. Convenience for tests and for replaying
/// a captured log; a live pipe wants [`EventStream`].
pub fn decode_all(text: &str) -> Vec<Result<AgentEvent, DecodeError>> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(decode_line)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanitised from a real `claude.exe` 2.1.228 capture: one prompt, two `Read`
    /// calls, `--include-partial-messages`. Paths, session ids and costs replaced;
    /// every structural feature — key order, nesting, unknown fields, the leading
    /// non-JSON warning — left exactly as captured.
    const TWO_TOOLS: &str = include_str!("../fixtures/claude_stream_two_tools.jsonl");

    /// Sanitised from a real persistent session: two human turns over one stdin,
    /// 25 seconds apart, `--replay-user-messages`. Two `result` objects, one session.
    const LIVE_SESSION: &str = include_str!("../fixtures/claude_stream_live_session.jsonl");

    /// Hand-written, not captured: the shapes the schema permits or a future CLI
    /// might send, which no capture on this machine happens to contain.
    const EDGES: &str = include_str!("../fixtures/claude_stream_edges.jsonl");

    fn events(text: &str) -> Vec<AgentEvent> {
        decode_all(text)
            .into_iter()
            .filter_map(Result::ok)
            .collect()
    }

    fn kinds(text: &str) -> Vec<EventKind> {
        events(text).into_iter().map(|event| event.kind).collect()
    }

    // -- the capture decodes, whole -----------------------------------------

    #[test]
    fn every_json_line_of_the_capture_decodes() {
        let outcomes = decode_all(TWO_TOOLS);
        let failures: Vec<_> = outcomes.iter().filter_map(|o| o.as_ref().err()).collect();
        // Exactly one line of the real capture is not JSON: the CLI's stdin warning.
        assert_eq!(failures.len(), 1, "unexpected failures: {failures:?}");
        match failures[0] {
            DecodeError::NotJson { line, .. } => {
                assert!(line.starts_with("Warning: no stdin data received"), "{line}");
            }
            other => panic!("expected NotJson, got {other:?}"),
        }
        assert_eq!(outcomes.len() - 1, events(TWO_TOOLS).len());
    }

    #[test]
    fn every_line_of_the_live_session_decodes() {
        let outcomes = decode_all(LIVE_SESSION);
        assert!(outcomes.iter().all(Result::is_ok), "{outcomes:?}");
        assert_eq!(outcomes.len(), 11);
    }

    #[test]
    fn a_non_json_line_is_reported_not_swallowed() {
        let error = decode_line("Warning: no stdin data received in 3s").unwrap_err();
        match error {
            DecodeError::NotJson { line, .. } => assert!(line.contains("no stdin data")),
            other => panic!("expected NotJson, got {other:?}"),
        }
    }

    // -- trap 1: `type` is not the first key --------------------------------

    #[test]
    fn the_result_decodes_although_type_is_not_the_first_key() {
        let line = TWO_TOOLS
            .lines()
            .find(|line| line.starts_with(r#"{"is_error""#))
            .expect("the capture's result line begins with is_error");
        assert!(
            line.find(r#""type":"result""#).unwrap() > line.len() - 120,
            "the fixture must keep `type` near the end, or this test proves nothing"
        );
        let event = decode_line(line).expect("decodes");
        let result = match event.kind {
            EventKind::Finished(result) => result,
            other => panic!("expected Finished, got {other:?}"),
        };
        assert!(!result.is_error);
        assert_eq!(result.subtype, "success");
        assert_eq!(result.num_turns, 3);
        assert_eq!(result.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(result.terminal_reason.as_deref(), Some("completed"));
        assert_eq!(result.duration_ms, Some(9639));
        assert_eq!(result.total_cost_usd, Some(0.0123));
        assert!(result.text.unwrap().starts_with("**fx-a.txt**"));
        assert_eq!(result.usage.unwrap().output_tokens, 404);
        assert!(result.api_error_status.is_none());
    }

    #[test]
    fn an_error_result_keeps_its_diagnosis() {
        let result = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Finished(result) => Some(result),
                _ => None,
            })
            .expect("the edge fixture has a failing result");
        assert!(result.is_error);
        assert_eq!(result.subtype, "error_during_execution");
        assert!(result.text.is_none());
        assert_eq!(result.api_error_status, Some(Value::from(500)));
        assert_eq!(result.permission_denials.len(), 1);
    }

    // -- trap 2: unknowns degrade, never fail -------------------------------

    #[test]
    fn an_unknown_event_type_survives_as_unknown() {
        let event = decode_line(
            r#"{"type":"quantum_entanglement_event","payload":{"spin":"up"},"uuid":"u-1"}"#,
        )
        .expect("an unknown type is not an error");
        assert_eq!(event.uuid.as_deref(), Some("u-1"));
        match event.kind {
            EventKind::Unknown { event_type, body } => {
                assert_eq!(event_type, "quantum_entanglement_event");
                // The whole line survives, `type` included.
                assert_eq!(body["payload"]["spin"], "up");
                assert_eq!(body["type"], "quantum_entanglement_event");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_system_subtype_survives_as_a_notice() {
        let notice = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Notice(notice) if notice.subtype == "weather_report" => Some(notice),
                _ => None,
            })
            .expect("the edge fixture has an invented subtype");
        assert_eq!(notice.field("forecast"), Some("rain"));
        assert_eq!(notice.needs_action(), Some("bring an umbrella"));
    }

    #[test]
    fn the_undocumented_post_turn_summary_is_readable() {
        let notice = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Notice(n) if n.subtype == "post_turn_summary" => Some(n),
                _ => None,
            })
            .expect("captured");
        assert_eq!(notice.status_category(), Some("review_ready"));
        assert_eq!(notice.status_detail(), Some("fx-a.txt: 3 lines; fx-b.txt: 2 lines"));
        // Empty on the wire; reported as absent rather than as an empty demand.
        assert_eq!(notice.needs_action(), None);
        assert!(notice.summarizes_uuid().is_some());
    }

    #[test]
    fn a_null_task_summary_detail_is_absent_not_empty() {
        let notice = kinds(TWO_TOOLS)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::Notice(n) if n.subtype == "task_summary" => Some(n),
                _ => None,
            })
            .last()
            .expect("the capture's trailing task_summary");
        assert_eq!(notice.detail(), None);
    }

    #[test]
    fn an_unknown_content_block_kind_survives() {
        let block = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Stream(StreamEvent::BlockStart { index: 3, block }) => Some(block),
                _ => None,
            })
            .expect("the edge fixture opens a server_tool_use block");
        match block {
            ContentBlock::Unknown { block_type, body } => {
                assert_eq!(block_type, "server_tool_use");
                assert_eq!(body["name"], "web_search");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_delta_kind_survives_with_its_index() {
        let (index, delta) = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Stream(StreamEvent::BlockDelta { index, delta }) => match delta {
                    Delta::Unknown { .. } => Some((index, delta)),
                    _ => None,
                },
                _ => None,
            })
            .expect("the edge fixture has a citations_delta");
        assert_eq!(index, 3);
        match delta {
            Delta::Unknown { delta_type, body } => {
                assert_eq!(delta_type, "citations_delta");
                assert_eq!(body["citation"]["title"], "Example");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_stream_event_survives() {
        let stream = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Stream(stream @ StreamEvent::Unknown { .. }) => Some(stream),
                _ => None,
            })
            .expect("the edge fixture has a message_pause");
        match stream {
            StreamEvent::Unknown { event_type, body } => {
                assert_eq!(event_type, "message_pause");
                assert_eq!(body["reason"], "awaiting_permission");
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn an_object_with_no_type_is_an_error_not_a_guess() {
        let error = decode_line(r#"{"subtype":"orphan","uuid":"u-1"}"#).unwrap_err();
        match error {
            DecodeError::NoType { body } => assert_eq!(body["subtype"], "orphan"),
            other => panic!("expected NoType, got {other:?}"),
        }
    }

    #[test]
    fn a_json_array_is_not_an_event() {
        let error = decode_line(r#"["not","an","object"]"#).unwrap_err();
        assert!(matches!(error, DecodeError::NotAnObject { .. }));
    }

    // -- trap 3: `user` is overloaded ---------------------------------------

    #[test]
    fn a_tool_result_user_line_is_not_human_input() {
        let turn = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::User(turn) => Some(turn),
                _ => None,
            })
            .expect("captured");
        assert_eq!(turn.voice(), UserVoice::ToolResponse);
        assert!(turn.human_text().is_empty());
        assert_eq!(turn.tool_results.len(), 1);
        let outcome = &turn.tool_results[0];
        assert_eq!(outcome.tool_use_id, "toolu_0000000000000000000001");
        assert!(!outcome.is_error);
        assert!(outcome.text().contains("alpha"));
        assert!(!turn.replay);
        // The undocumented sibling, which carries what the text does not.
        let structured = turn.tool_use_result.as_ref().expect("captured");
        assert_eq!(structured["file"]["totalLines"], 4);
    }

    #[test]
    fn a_replayed_human_line_is_human_input() {
        let turn = kinds(LIVE_SESSION)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::User(turn) => Some(turn),
                _ => None,
            })
            .expect("the live session replays its human turns");
        assert_eq!(turn.voice(), UserVoice::Human);
        assert_eq!(turn.human_text(), "Reply with exactly: TURN-ONE");
        assert!(turn.tool_results.is_empty());
        assert!(turn.replay, "isReplay marks the echo of injected input");
    }

    #[test]
    fn the_bare_string_form_of_a_user_message_is_human_input() {
        let turn = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::User(turn) if turn.human_text().starts_with("just a plain") => {
                    Some(turn)
                }
                _ => None,
            })
            .expect("the edge fixture has the bare-string form");
        assert_eq!(turn.voice(), UserVoice::Human);
    }

    #[test]
    fn a_mixed_user_line_keeps_both_halves_and_says_so() {
        let turn = kinds(EDGES)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::User(turn) if turn.voice() == UserVoice::Mixed => Some(turn),
                _ => None,
            })
            .expect("the edge fixture mixes a tool result with human text");
        assert_eq!(turn.tool_results.len(), 1);
        assert_eq!(turn.human_text(), "and here is a human aside");
        // The image block is neither, and is kept rather than dropped.
        assert_eq!(turn.other.len(), 1);
        assert_eq!(turn.other[0]["type"], "image");
    }

    #[test]
    fn a_failed_tool_result_reports_its_failure_and_its_block_content() {
        let outcome = kinds(EDGES)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::User(turn) => Some(turn),
                _ => None,
            })
            .flat_map(|turn| turn.tool_results)
            .find(|outcome| outcome.is_error)
            .expect("the edge fixture has a failing tool result");
        assert_eq!(outcome.tool_use_id, "toolu_0000000000000000000202");
        // Array-of-blocks form, flattened by `text()`.
        assert_eq!(outcome.text(), "File does not exist.");
    }

    // -- tool calls ---------------------------------------------------------

    #[test]
    fn a_tool_calls_name_and_complete_input_are_recovered() {
        let calls: Vec<ToolCall> = kinds(TWO_TOOLS)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::Assistant(turn) => Some(turn),
                _ => None,
            })
            .flat_map(|turn| turn.tool_calls().cloned().collect::<Vec<_>>())
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "Read");
        assert_eq!(calls[0].id, "toolu_0000000000000000000001");
        assert_eq!(calls[0].input["file_path"], "C:\\work\\demo\\fx-a.txt");
        assert_eq!(calls[1].input["file_path"], "C:\\work\\demo\\fx-b.txt");
    }

    #[test]
    fn a_streaming_tool_calls_input_is_empty_until_the_fragments_land() {
        let block = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Stream(StreamEvent::BlockStart { index: 1, block }) => Some(block),
                _ => None,
            })
            .expect("captured");
        match block {
            ContentBlock::ToolUse(call) => {
                assert_eq!(call.name, "Read");
                assert_eq!(call.input, serde_json::json!({}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn concatenated_input_fragments_parse_as_the_settled_input() {
        let mut fragments = String::new();
        for kind in kinds(TWO_TOOLS) {
            if let EventKind::Stream(StreamEvent::BlockDelta {
                index: 1,
                delta: Delta::ToolInputJson(piece),
            }) = kind
            {
                fragments.push_str(&piece);
            }
        }
        let parsed: Value = serde_json::from_str(&fragments).expect("fragments form one JSON doc");
        assert_eq!(parsed["file_path"], "C:\\work\\demo\\fx-a.txt");
    }

    // -- trap 4: deltas and settled messages, both, labelled ----------------

    #[test]
    fn deltas_are_attributed_to_their_own_content_block() {
        let mut by_block: Vec<(usize, String)> = Vec::new();
        for kind in kinds(TWO_TOOLS) {
            if let EventKind::Stream(StreamEvent::BlockDelta { index, delta }) = kind {
                let text = match delta {
                    Delta::Text(text) => format!("text:{text}"),
                    Delta::ToolInputJson(_) => "json".to_string(),
                    other => format!("{other:?}"),
                };
                by_block.push((index, text));
            }
        }
        // Block 0 is prose, blocks 1 and 2 are tool arguments — never mixed.
        assert!(by_block
            .iter()
            .filter(|(index, _)| *index == 0)
            .all(|(_, text)| text.starts_with("text:")));
        assert!(by_block
            .iter()
            .filter(|(index, _)| *index > 0)
            .all(|(_, text)| text == "json"));
        assert_eq!(
            by_block
                .iter()
                .filter(|(index, text)| *index == 1 && text == "json")
                .count(),
            4
        );
    }

    #[test]
    fn an_index_less_delta_is_not_attributed_to_a_guess() {
        let stream: Vec<StreamEvent> = kinds(EDGES)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::Stream(stream) => Some(stream),
                _ => None,
            })
            .collect();
        // The malformed delta must not appear as BlockDelta { index: 0, .. }.
        assert!(!stream.iter().any(|event| matches!(
            event,
            StreamEvent::BlockDelta {
                index: 0,
                delta: Delta::Text(text)
            } if text.contains("index-less")
        )));
        assert!(stream.iter().any(|event| matches!(
            event,
            StreamEvent::Unknown { event_type, .. } if event_type == "content_block_delta"
        )));
    }

    #[test]
    fn both_the_deltas_and_the_settled_message_are_reported() {
        let mut streamed = String::new();
        let mut settled = String::new();
        for kind in kinds(TWO_TOOLS) {
            match kind {
                EventKind::Stream(StreamEvent::BlockDelta {
                    index: 0,
                    delta: Delta::Text(piece),
                }) => streamed.push_str(&piece),
                EventKind::Assistant(turn) => settled.push_str(&turn.text()),
                _ => {}
            }
        }
        assert!(streamed.starts_with("I'll read both files."));
        assert!(settled.starts_with("I'll read both files."));
    }

    #[test]
    fn assistant_lines_of_one_message_share_a_message_id() {
        let ids: Vec<String> = kinds(TWO_TOOLS)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::Assistant(turn) => turn.message_id,
                _ => None,
            })
            .collect();
        // Three lines for the first message (text, tool_use, tool_use), one for the
        // second — the settled view is per content block, not per message.
        assert_eq!(ids.len(), 4);
        assert_eq!(ids[0], ids[1]);
        assert_eq!(ids[1], ids[2]);
        assert_ne!(ids[2], ids[3]);
    }

    #[test]
    fn the_real_output_token_count_arrives_on_the_message_delta() {
        let settled = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Assistant(turn) => turn.usage,
                _ => None,
            })
            .expect("captured");
        let delta = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::Stream(StreamEvent::MessageDelta { usage, .. }) => usage,
                _ => None,
            })
            .expect("captured");
        assert_eq!(settled.output_tokens, 1, "the assistant line's is a placeholder");
        assert_eq!(delta.output_tokens, 309);
    }

    // -- trap 5: `result` terminates a turn, not the stream -----------------

    #[test]
    fn events_follow_the_result_in_a_one_shot_run() {
        let kinds = kinds(TWO_TOOLS);
        let at = kinds
            .iter()
            .position(|kind| matches!(kind, EventKind::Finished(_)))
            .expect("captured");
        assert!(at < kinds.len() - 1, "a system line follows the result");
        assert!(matches!(kinds[at + 1], EventKind::Notice(_)));
    }

    #[test]
    fn a_persistent_session_emits_one_result_per_turn() {
        let kinds = kinds(LIVE_SESSION);
        let results: Vec<&TurnResult> = kinds
            .iter()
            .filter_map(|kind| match kind {
                EventKind::Finished(result) => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2, "two turns, two results, one stream");
        assert_eq!(results[0].text.as_deref(), Some("TURN-ONE"));
        assert_eq!(results[1].text.as_deref(), Some("TURN-TWO"));
        // num_turns counts the run, not the session: 1 on both, not 1 then 2.
        assert_eq!(results[0].num_turns, 1);
        assert_eq!(results[1].num_turns, 1);
        // More turns follow the first result.
        let first = kinds
            .iter()
            .position(|kind| matches!(kind, EventKind::Finished(_)))
            .unwrap();
        assert!(kinds[first + 1..]
            .iter()
            .any(|kind| matches!(kind, EventKind::Assistant(_))));
    }

    #[test]
    fn one_session_id_spans_every_turn() {
        let ids: std::collections::BTreeSet<String> = events(LIVE_SESSION)
            .into_iter()
            .filter_map(|event| event.session_id)
            .collect();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn session_init_recurs_mid_stream() {
        let starts: Vec<SessionStart> = kinds(LIVE_SESSION)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::SessionStarted(start) => Some(start),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 2, "init is re-announced before turn two");
        assert_eq!(starts[0].cwd, starts[1].cwd);
    }

    // -- the session announcement -------------------------------------------

    #[test]
    fn the_session_announcement_is_typed() {
        let start = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::SessionStarted(start) => Some(start),
                _ => None,
            })
            .expect("captured");
        assert_eq!(start.cwd, "C:\\work\\demo");
        assert_eq!(start.model, "claude-opus-5[1m]");
        assert_eq!(start.cli_version, "2.1.228");
        assert_eq!(start.permission_mode, "default", "camelCase on the wire");
        assert!(start.tools.contains(&"Read".to_string()));
        assert_eq!(start.mcp_servers.len(), 2);
        assert_eq!(start.mcp_servers[0].status, "connected");
        assert_eq!(start.mcp_servers[1].status, "pending");
    }

    #[test]
    fn the_rate_limit_event_is_typed_through_its_camel_case_keys() {
        let limit = kinds(TWO_TOOLS)
            .into_iter()
            .find_map(|kind| match kind {
                EventKind::RateLimit(limit) => Some(limit),
                _ => None,
            })
            .expect("captured");
        assert_eq!(limit.status, "allowed");
        assert_eq!(limit.resets_at, Some(1_786_573_800));
        assert_eq!(limit.limit_type.as_deref(), Some("five_hour"));
        assert!(!limit.using_overage);
    }

    // -- scope --------------------------------------------------------------

    #[test]
    fn a_null_parent_tool_use_id_means_the_main_conversation() {
        for event in events(TWO_TOOLS) {
            assert_eq!(event.scope, AgentScope::Main, "{:?}", event.kind);
            assert!(!event.is_from_subagent());
        }
    }

    #[test]
    fn a_present_parent_tool_use_id_names_the_subagent() {
        let event = events(EDGES)
            .into_iter()
            .find(|event| event.is_from_subagent())
            .expect("the edge fixture has a subagent line");
        assert_eq!(
            event.subagent_tool_use_id(),
            Some("toolu_0000000000000000000203")
        );
    }

    // -- trap 6: partial lines ----------------------------------------------

    #[test]
    fn a_split_line_cannot_produce_a_wrong_decode() {
        let whole = events(TWO_TOOLS);
        // Every split point that matters: one byte at a time is the worst case.
        for chunk_size in [1usize, 2, 7, 64, 997, 8192] {
            let mut stream = EventStream::new();
            let mut decoded = Vec::new();
            for chunk in TWO_TOOLS.as_bytes().chunks(chunk_size) {
                decoded.extend(stream.push(chunk).into_iter().filter_map(Result::ok));
            }
            decoded.extend(stream.flush().into_iter().filter_map(Result::ok));
            assert_eq!(decoded, whole, "chunk size {chunk_size} changed the answer");
        }
    }

    #[test]
    fn a_partial_line_yields_nothing_at_all() {
        let mut stream = EventStream::new();
        let line = r#"{"type":"system","subtype":"status","status":"requesting"}"#;
        let split = line.len() - 10;
        assert!(stream.push(line[..split].as_bytes()).is_empty());
        assert!(stream.pending_bytes() > 0);
        assert!(stream.push(line[split..].as_bytes()).is_empty());
        let events = stream.push(b"\n");
        assert_eq!(events.len(), 1);
        assert_eq!(stream.pending_bytes(), 0);
    }

    #[test]
    fn a_multibyte_character_split_across_reads_survives() {
        // The capture's prose contains an em dash; split it down the middle.
        let line = r#"{"type":"system","subtype":"task_summary","detail":"a — b"}"#;
        let bytes = line.as_bytes();
        let dash = line.find('—').unwrap();
        let mut stream = EventStream::new();
        assert!(stream.push(&bytes[..dash + 1]).is_empty());
        let mut events = stream.push(&bytes[dash + 1..]);
        events.extend(stream.push(b"\n"));
        assert_eq!(events.len(), 1);
        match events.remove(0).unwrap().kind {
            EventKind::Notice(notice) => assert_eq!(notice.detail(), Some("a — b")),
            other => panic!("expected Notice, got {other:?}"),
        }
    }

    #[test]
    fn crlf_line_endings_decode() {
        let mut stream = EventStream::new();
        let events = stream.push(b"{\"type\":\"stream_event\",\"event\":{\"type\":\"message_stop\"}}\r\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].as_ref().unwrap().kind,
            EventKind::Stream(StreamEvent::MessageStop)
        ));
    }

    #[test]
    fn blank_lines_are_dropped_rather_than_reported() {
        let mut stream = EventStream::new();
        assert!(stream.push(b"\n\n   \n").is_empty());
        assert_eq!(stream.pending_bytes(), 0);
    }

    #[test]
    fn a_final_line_without_a_newline_is_flushed_at_eof() {
        let mut stream = EventStream::new();
        assert!(stream
            .push(br#"{"type":"stream_event","event":{"type":"message_stop"}}"#)
            .is_empty());
        let event = stream.flush().expect("flushed").expect("decodes");
        assert!(matches!(
            event.kind,
            EventKind::Stream(StreamEvent::MessageStop)
        ));
        assert!(stream.flush().is_none());
    }

    #[test]
    fn a_long_line_survives_being_reassembled() {
        // A tool_result carrying a file is the realistic case for a split.
        let payload = "x".repeat(200_000);
        let line = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "toolu_1", "content": payload }
            ]}
        })
        .to_string();
        let mut stream = EventStream::new();
        let mut events = Vec::new();
        for chunk in line.as_bytes().chunks(4096) {
            events.extend(stream.push(chunk));
        }
        assert!(events.is_empty(), "no newline, no event");
        let event = stream.flush().expect("flushed").expect("decodes");
        match event.kind {
            EventKind::User(turn) => assert_eq!(turn.tool_results[0].text().len(), 200_000),
            other => panic!("expected User, got {other:?}"),
        }
    }

    // -- thinking -----------------------------------------------------------

    #[test]
    fn thinking_blocks_and_their_deltas_are_typed() {
        let stream: Vec<StreamEvent> = kinds(EDGES)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::Stream(stream) => Some(stream),
                _ => None,
            })
            .collect();
        assert!(stream.iter().any(|event| matches!(
            event,
            StreamEvent::BlockStart {
                block: ContentBlock::Thinking { .. },
                ..
            }
        )));
        assert!(stream.iter().any(|event| matches!(
            event,
            StreamEvent::BlockDelta { delta: Delta::Thinking(text), .. }
                if text.starts_with("Let me weigh")
        )));
        assert!(stream
            .iter()
            .any(|event| matches!(event, StreamEvent::BlockDelta { delta: Delta::Signature(_), .. })));
    }

    // -- error rendering ----------------------------------------------------

    #[test]
    fn a_decode_error_message_does_not_quote_a_whole_file() {
        let huge = format!("not json {}", "y".repeat(50_000));
        let error = decode_line(&huge).unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.len() < 400, "{} bytes", rendered.len());
        assert!(rendered.contains("50009 bytes"));
    }
}

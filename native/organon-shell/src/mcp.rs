//! The console's MCP server: its capabilities as tools, and its answer to "may I?".
//!
//! Two things live here, because Claude Code makes them one process. The console runs a
//! single stdio MCP server that serves:
//!
//! 1. **The capability tools** — every [`CommandSpec`] in the console's command table,
//!    rendered as an MCP tool with a JSON Schema *generated* from the table
//!    ([`input_schema`]). Never a hand-written second copy: this tree has already paid
//!    for one of those at nine wrong ranges out of forty-five, with the wrong bounds
//!    shipped in published documentation.
//! 2. **The permission handler** — the tool `--permission-prompt-tool` points at, which
//!    Claude Code consults for *every* tool the agent calls, its own MCP tools and
//!    `Bash` alike. That is what lets an approval arrive as a card in the conversation.
//!
//! It is a **library module, not a binary**. [`McpServer::handle`] takes one decoded
//! message and returns at most one response; [`McpServer::push`] adds the byte framing;
//! [`McpServer::serve`] adds the two pipes. The tests drive the first of those, so none
//! of them spawns a process or touches stdio.
//!
//! # Measured, not assumed
//!
//! Every wire shape below is quoted from `doc/console_approval_protocol.md`, which
//! measured them against `claude.exe` 2.1.228 rather than reading them from
//! documentation. The findings that changed the code:
//!
//! - **The startup handshake happens twice, in two processes.** The first process calls
//!   `server/discover` at `protocolVersion 2026-07-28`; a `-32601` is the correct reply,
//!   after which the client **respawns** us and performs a normal `initialize` at
//!   `2025-11-25`. So an unknown method must be answered and survived
//!   ([`McpServer::handle`] never latches an error state), and a cold second start must
//!   be indistinguishable from a first — which it is, because a server owns no
//!   cross-process state.
//! - **The permission request is three flat arguments** — `tool_name`, `input`,
//!   `tool_use_id` — and the answer is a JSON **string inside ordinary MCP text
//!   content**, not a structured result. [`PermissionDecision::to_wire`] emits exactly
//!   the two payloads the doc quotes, byte for byte, and a test pins them.
//! - **`updatedInput` is mandatory on an allow** and is re-validated against the called
//!   tool's schema. It is not ceremony: it is the seam that lets a card offer *"allow,
//!   but not that path"*. [`PermissionDecision::Allow`] therefore carries a value the
//!   responder chooses, and [`PermissionDecision::allow_unchanged`] is the echo case.
//! - **Tools that request user interaction are rejected outright** by the binary ("MCP
//!   tool requires user interaction; not supported via --permission-prompt-tool"). So
//!   nothing here uses MCP elicitation. The human is reached through the console's own
//!   UI, behind the synchronous [`PermissionResponder`] hook.
//! - **stdout carries JSON-RPC only.** [`McpServer::serve`] writes responses and nothing
//!   else, and this module never prints. Logging is the caller's, on another stream.
//!
//! # What this module does not decide
//!
//! **The verdict.** [`PermissionResponder`] is supplied by the caller, because the
//! console answers from a card the human clicked and from its own remembered decisions
//! — and remembering is ours to build (the doc measured three identical calls producing
//! three separate handler requests; there is no upstream persistence). This module owns
//! the protocol; the judgement belongs to whoever implements the hook.
//!
//! ⚠️ The hook is **synchronous and may block for as long as a human takes**. It is
//! called on whatever thread drives [`McpServer::serve`], and the client is simply
//! waiting on a JSON-RPC response meanwhile. That is the intended shape — a card with
//! no timeout — but it means the serve loop must not be the UI thread.
//!
//! That last sentence is why the responder is `Box<dyn PermissionResponder + Send>` and
//! why the whole server is `Send`: [`crate::mcp_http`] moves one onto a serve thread and
//! the console's hook posts to the UI over a channel. The bound is on the *box*, not on
//! the trait, so a test can still implement the trait with anything it likes as long as
//! what it hands over may cross a thread.
//!
//! # Why dispatch is a parameter, not a field
//!
//! [`CommandService`] borrows the session log mutably for its whole life, so a server
//! that *stored* one would hold that borrow for the life of the server. Instead every
//! entry point takes `&mut dyn ToolDispatch`, and [`ServiceDispatch`] adapts a service
//! at the call site. The server stays plain data plus a hook.

use std::io::{self, Read, Write};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::command::{ArgKind, CommandService, CommandSpec};
use crate::session::Issuer;

// ---------------------------------------------------------------------------
// Constants — the vocabulary of the wire
// ---------------------------------------------------------------------------

/// The MCP revision we speak natively. Sent when the client asks for something we do
/// not recognise, per the spec's "answer with your latest".
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// Revisions we will echo back rather than override. `2025-11-25` is the one the
/// measured client negotiated; the older three are wire-compatible for the four methods
/// we implement. `2026-07-28` is deliberately absent — that is the `server/discover`
/// probe's version, and answering it would claim a revision we have never seen.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Default `serverInfo.name`. Claude Code namespaces our tools as
/// `mcp__<this>__<tool_name>`, which is the string `--allowedTools` and
/// `--permission-prompt-tool` are spelled with.
pub const DEFAULT_SERVER_NAME: &str = "organon";

/// Default name of the permission handler tool — so the flag reads
/// `--permission-prompt-tool mcp__organon__approve_tool`.
pub const DEFAULT_PERMISSION_TOOL: &str = "approve_tool";

/// JSON-RPC 2.0 error codes, spelled once.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

// ---------------------------------------------------------------------------
// The dispatch seam
// ---------------------------------------------------------------------------

/// Where a `tools/call` goes once the server has resolved the tool name back to a
/// command name. `Err` is a *tool* failure (the model should see it and may retry),
/// never a protocol failure — the server has already established that the tool exists.
pub trait ToolDispatch {
    fn call(&mut self, command: &str, args: Value) -> Result<Value, String>;
}

/// A dispatch for a server that serves **no capability tools**, only the permission
/// handler.
///
/// It is not a stub standing in for something missing: with an empty spec table
/// [`McpServer::call_tool`] refuses an unknown tool as `INVALID_PARAMS` before dispatch is
/// consulted, so nothing can reach this. It exists so the type is inhabited and the
/// omission is named rather than implied.
///
/// ⚠️ **This is no longer what the console runs** — a conversation tab is handed a real
/// spec table and a real dispatch ([`crate::conversation_view::Capabilities`]). It remains
/// the honest answer for a caller with no capabilities to offer, and for the tests of every
/// path that does not need one.
pub struct NoDispatch;

impl ToolDispatch for NoDispatch {
    fn call(&mut self, command: &str, _args: Value) -> Result<Value, String> {
        Err(format!(
            "'{command}' is not routed over MCP: this server serves the permission handler only"
        ))
    }
}

/// The production adapter: a [`CommandService`] plus the [`Issuer`] every dispatch is
/// recorded under. Constructed per call so the service's borrow of the session log
/// never has to outlive the statement that uses it.
///
/// Validation and the audit record both come for free — `CommandService::dispatch`
/// checks arguments against the same [`CommandSpec`] this server generated the schema
/// from, and leaves a `CommandRun` record whether it passed or not.
pub struct ServiceDispatch<'svc, 'log> {
    service: &'svc mut CommandService<'log>,
    issuer: Issuer,
}

impl<'svc, 'log> ServiceDispatch<'svc, 'log> {
    pub fn new(service: &'svc mut CommandService<'log>, issuer: Issuer) -> Self {
        Self { service, issuer }
    }
}

impl ToolDispatch for ServiceDispatch<'_, '_> {
    fn call(&mut self, command: &str, args: Value) -> Result<Value, String> {
        self.service
            .dispatch(self.issuer.clone(), command, args)
            .map(|outcome| outcome.result)
            .map_err(|err| err.to_string())
    }
}

// ---------------------------------------------------------------------------
// The permission hook
// ---------------------------------------------------------------------------

/// One approval question, exactly as the client asks it (doc §3). Three flat arguments
/// and nothing else: `tool_use_id` is what lets a card attach to the tool element
/// already in the transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    /// The *namespaced* name of the tool being gated — `Bash`, `mcp__organon__…`. Not
    /// necessarily one of ours: the handler answers for everything the agent calls.
    pub tool_name: String,
    /// The arguments the model proposed. Echo or rewrite them via
    /// [`PermissionDecision::Allow`].
    pub input: Value,
    /// The `toolu_…` id, for correlating the card with the transcript.
    pub tool_use_id: String,
}

/// The verdict (doc §4).
///
/// ⚠️ `updated_input` is **mandatory** on an allow and is re-validated against the
/// called tool's schema, so it must be a JSON **object** shaped like that tool's
/// arguments. Nothing here rewrites it for you — a silently corrected payload would
/// hide a caller bug behind an upstream validation error.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow { updated_input: Value },
    Deny { message: String },
}

impl PermissionDecision {
    /// The ordinary allow: the model's own arguments, unchanged.
    pub fn allow_unchanged(request: &PermissionRequest) -> Self {
        PermissionDecision::Allow { updated_input: request.input.clone() }
    }

    /// Allow, but with these arguments instead — the "allow, but not that path" card.
    pub fn allow_with(updated_input: Value) -> Self {
        PermissionDecision::Allow { updated_input }
    }

    pub fn deny(message: impl Into<String>) -> Self {
        PermissionDecision::Deny { message: message.into() }
    }

    /// The payload as it goes on the wire — a JSON string that is then wrapped in
    /// ordinary MCP text content. Pinned byte-for-byte against the doc's quotes by
    /// `allow_payload_is_byte_exact` / `deny_payload_is_byte_exact`.
    pub fn to_wire(&self) -> String {
        #[derive(Serialize)]
        #[serde(tag = "behavior", rename_all = "lowercase")]
        enum Wire<'a> {
            Allow {
                #[serde(rename = "updatedInput")]
                updated_input: &'a Value,
            },
            Deny {
                message: &'a str,
            },
        }
        let wire = match self {
            PermissionDecision::Allow { updated_input } => Wire::Allow { updated_input },
            PermissionDecision::Deny { message } => Wire::Deny { message },
        };
        // Infallible: `Wire` is a map of JSON-representable fields.
        serde_json::to_string(&wire).unwrap_or_else(|_| String::from("{\"behavior\":\"deny\",\"message\":\"internal serialisation failure\"}"))
    }
}

// -- the deadline, and what holds it off ------------------------------------

/// 🚨 **How long Claude Code waits for a `--permission-prompt-tool` answer before it gives
/// up.** Measured 2026-08-12 on `claude.exe` 2.1.228, twice in one run against a probe
/// server that deliberately never answered: **60.010 s** and **60.005 s** from the
/// `tools/call` arriving to the client aborting the socket (`WSAECONNRESET`). The model
/// then sees `Error calling tool (Write): The operation timed out.`
///
/// ⚠️ **The design this replaced assumed there was no deadline** — "the hook blocks, the
/// client just waits". It does not, and a human reading an approval card for a minute is
/// not unusual, so the assumption failed the first time a real person used the feature.
pub const CLIENT_DEADLINE_SECS: u64 = 60;

/// How often a pending approval emits `notifications/progress` to hold the deadline open.
///
/// **Measured: progress against the request's own `progressToken` resets the clock.** The
/// same probe, answering after **300 s** while beating every 10 s, was answered and the
/// write went through — five times the deadline, with no abort.
///
/// 10 s is a sixth of [`CLIENT_DEADLINE_SECS`]. The margin is the point: a beat is written
/// from the thread that is *blocked on the human*, so it competes with nothing, but the
/// socket write can still stall, and five consecutive beats would have to be lost before
/// the client gave up. A cadence near the deadline would make one slow write fatal; a much
/// faster one would be traffic for its own sake.
pub const HEARTBEAT: Duration = Duration::from_secs(10);

/// What a blocking [`PermissionResponder`] calls while it waits, so the client does not
/// give up on it.
///
/// **It answers one question and performs one action**: emit a keep-alive, and say whether
/// the client is still on the other end. A responder that blocks must stop the moment this
/// returns `false` — the request it is answering no longer exists, and a card still asking
/// about it is a question whose answer cannot matter.
///
/// The transport implements it ([`crate::mcp_http`]) because holding a connection open is a
/// transport concern; this module only states the contract, and [`NoHeartbeat`] is the
/// honest answer for a caller with no connection to keep alive.
pub trait Heartbeat {
    /// Emit one keep-alive. `false` means the client has gone — stop waiting.
    fn beat(&mut self) -> bool;
}

/// The heartbeat for a call nobody is streaming: nothing to emit, nobody to lose.
///
/// ⚠️ Used by [`McpServer::handle`] and by every test. It reports the client is still there
/// **forever**, which is correct for a caller that is not on a real socket and would be a
/// silent wedge for one that is — which is why the HTTP transport never uses it for a
/// permission call.
pub struct NoHeartbeat;

impl Heartbeat for NoHeartbeat {
    fn beat(&mut self) -> bool {
        true
    }
}

/// The caller's judgement. The console implements this over its approval cards and its
/// own decision memory; a test implements it as a closure.
///
/// See the module doc: this is called synchronously on the serve thread and may block. It
/// is handed a [`Heartbeat`] precisely *because* it may block — see [`HEARTBEAT`].
pub trait PermissionResponder {
    fn decide(&mut self, request: &PermissionRequest, beat: &mut dyn Heartbeat)
        -> PermissionDecision;
}

impl<F> PermissionResponder for F
where
    F: FnMut(&PermissionRequest) -> PermissionDecision,
{
    /// A closure answers at once, so it never needs the heartbeat. Keeping the blanket
    /// impl is what lets every existing test stay a one-line closure.
    fn decide(
        &mut self,
        request: &PermissionRequest,
        _beat: &mut dyn Heartbeat,
    ) -> PermissionDecision {
        self(request)
    }
}

// ---------------------------------------------------------------------------
// Tools, generated from the command table
// ---------------------------------------------------------------------------

/// One capability tool, and the command it forwards to.
///
/// `tool_name` and `command_name` differ whenever the command name contains a character
/// MCP tool names may not — see [`tool_name_for`]. Keeping both is what lets the server
/// resolve a call back to the catalog without a second table.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolEntry {
    pub tool_name: String,
    pub command_name: String,
    pub description: String,
    pub input_schema: Value,
}

impl ToolEntry {
    /// Generated from a spec — the only way one is made.
    pub fn from_spec(spec: &CommandSpec) -> Self {
        Self {
            tool_name: tool_name_for(&spec.name),
            command_name: spec.name.clone(),
            description: spec.doc.clone(),
            input_schema: input_schema(spec),
        }
    }

    /// The `tools/list` element.
    pub fn definition(&self) -> Value {
        json!({
            "name": self.tool_name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }
}

/// A command name made legal as an MCP tool name.
///
/// ⚠️ **The console's command names are dotted (`session.note`) and MCP tool names are
/// not allowed to be.** Anthropic's tool-name grammar is `^[a-zA-Z0-9_-]{1,128}$`, so a
/// dot would be rejected at the API boundary rather than here, far from the cause. Every
/// character outside that set becomes `_`; the mapping back is kept in [`ToolEntry`], so
/// nothing downstream has to guess.
///
/// The transform is many-to-one, so it can collide — [`McpServer::name_collisions`]
/// reports collisions instead of letting one command silently shadow another.
pub fn tool_name_for(command: &str) -> String {
    let mut out: String = command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    out.truncate(128);
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// The JSON Schema for a spec's arguments — **generated, never written twice**.
///
/// The mapping, one line per [`ArgKind`]:
///
/// | `ArgKind` | JSON Schema |
/// |---|---|
/// | `Float { min, max }` | `{"type":"number","minimum":min,"maximum":max}` |
/// | `Int` | `{"type":"integer"}` |
/// | `Bool` | `{"type":"boolean"}` |
/// | `Text` | `{"type":"string"}` |
/// | `Choice(v)` | `{"type":"string","enum":v}` |
///
/// Two places where the two vocabularies do not line up, both handled here rather than
/// left to surprise someone:
///
/// - ⚠️ **A non-finite bound is dropped, not emitted.** `serde_json` cannot represent
///   `f64::INFINITY`, and serialises it as `null` — so an unbounded `Float` would ship
///   `"minimum": null`, which is not a schema, in a document a model reads. Only finite
///   bounds are written; an unbounded axis simply carries no `minimum`/`maximum`, which
///   is what "unbounded" means in JSON Schema anyway.
/// - **`additionalProperties` is `true`, deliberately.** `command.rs` tolerates
///   undeclared arguments and passes them to the target; a closed schema here would
///   describe a stricter service than the one that actually runs. The schema is the
///   model's guide, and `CommandService::dispatch` remains the gate.
///
/// `minimum`/`maximum` are inclusive in JSON Schema, matching `check_kind`'s
/// `v >= min && v <= max` exactly.
pub fn input_schema(spec: &CommandSpec) -> Value {
    let mut properties = Map::new();
    let mut required: Vec<Value> = Vec::new();
    for arg in &spec.args {
        properties.insert(arg.name.clone(), arg_schema(&arg.kind));
        if arg.required {
            required.push(Value::String(arg.name.clone()));
        }
    }
    let mut schema = Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    schema.insert("additionalProperties".into(), Value::Bool(true));
    Value::Object(schema)
}

fn arg_schema(kind: &ArgKind) -> Value {
    match kind {
        ArgKind::Float { min, max } => {
            let mut map = Map::new();
            map.insert("type".into(), Value::String("number".into()));
            // See `input_schema`: a non-finite bound would serialise as `null`.
            if let Some(n) = serde_json::Number::from_f64(*min) {
                map.insert("minimum".into(), Value::Number(n));
            }
            if let Some(n) = serde_json::Number::from_f64(*max) {
                map.insert("maximum".into(), Value::Number(n));
            }
            Value::Object(map)
        }
        ArgKind::Int => json!({ "type": "integer" }),
        ArgKind::Bool => json!({ "type": "boolean" }),
        ArgKind::Text => json!({ "type": "string" }),
        ArgKind::Choice(options) => json!({ "type": "string", "enum": options }),
    }
}

/// The permission handler's own schema.
///
/// All three arguments are declared required because all three were present in every
/// measured capture. The *runtime* handler is more liberal than its own schema — a
/// missing `tool_use_id` yields an empty string rather than an error — because a card
/// that cannot correlate is still better than an approval that cannot be answered.
fn permission_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tool_name": { "type": "string" },
            "input": { "type": "object" },
            "tool_use_id": { "type": "string" }
        },
        "required": ["tool_name", "input", "tool_use_id"],
        "additionalProperties": true
    })
}

// ---------------------------------------------------------------------------
// The exposure audit — §7's security property, checked per session
// ---------------------------------------------------------------------------

/// What the session says the model can actually see, checked against what this server
/// serves. Built by [`McpServer::audit_exposure`]; the sentence a human reads is
/// [`ExposureAudit::summary`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExposureAudit {
    /// How many tools the session reported in total. **Zero means the audit proved
    /// nothing**, which is a different fact from "the handler is absent".
    pub reported: usize,
    /// 🚨 **The failure.** The permission handler is in the model's own tool list, so the
    /// model can call it and answer its own approvals. §7 measured that Claude Code
    /// withholds it; if this is ever true, that guarantee has stopped holding for this
    /// server and the console's authority is decorative.
    pub handler_offered: bool,
    /// The console's verbs the model can see — the point of serving them at all.
    pub capabilities_offered: Vec<String>,
    /// Served here, absent from the model's list. Not a fault by itself: MCP tools arrive
    /// **deferred** in the measured build, so a name can be reachable without being
    /// preloaded. Reported because "the agent could not find our tool" and "we never served
    /// it" are otherwise indistinguishable from the outside.
    pub capabilities_withheld: Vec<String>,
}

impl ExposureAudit {
    /// The audit from three plain lists, so a caller that no longer owns the server can
    /// still make it.
    ///
    /// ⚠️ [`McpServer::audit_exposure`] is a thin wrapper over this rather than a second
    /// implementation: the console hands its server to the transport thread and keeps only
    /// the two name lists, so both callers must reach the same arithmetic or the audit and
    /// the thing it audits could come to disagree.
    pub fn of(handler: &str, served: &[String], offered: &[String]) -> Self {
        let visible = |name: &String| offered.iter().any(|t| t == name);
        Self {
            reported: offered.len(),
            handler_offered: offered.iter().any(|t| t == handler),
            capabilities_offered: served.iter().filter(|n| visible(n)).cloned().collect(),
            capabilities_withheld: served.iter().filter(|n| !visible(n)).cloned().collect(),
        }
    }

    /// Did the session report a tool list at all? Everything else here is only meaningful
    /// when this is true.
    pub fn reported_anything(&self) -> bool {
        self.reported > 0
    }

    /// One line for the log, phrased so that the dangerous case cannot read as the safe
    /// one. **Says what did not work as plainly as what did** — an audit against an empty
    /// list announces that it proved nothing rather than staying silent.
    pub fn summary(&self) -> String {
        if !self.reported_anything() {
            return "the session reported no tools — the approval handler's exposure could \
                    not be checked this init"
                .to_string();
        }
        if self.handler_offered {
            return format!(
                "🚨 the approval handler is in the model's own tool list ({} tools offered) — \
                 it can answer its own permission requests; do not trust this session's cards",
                self.reported
            );
        }
        format!(
            "approvals: handler withheld from the model as measured, {} of {} console tools \
             visible ({} offered)",
            self.capabilities_offered.len(),
            self.capabilities_offered.len() + self.capabilities_withheld.len(),
            self.reported
        )
    }
}

// ---------------------------------------------------------------------------
// The server
// ---------------------------------------------------------------------------

struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// The stdio JSON-RPC server, as a value.
///
/// It holds a generated tool table, the permission tool's name, and the caller's
/// decision hook — and no connection, no thread and no process. Feed it messages
/// ([`McpServer::handle`]), bytes ([`McpServer::push`]) or a pair of pipes
/// ([`McpServer::serve`]).
pub struct McpServer {
    server_name: String,
    server_version: String,
    permission_tool: String,
    /// The table the tools were generated from, kept so a later rename can re-resolve
    /// it exactly rather than from the lossy served set.
    specs: Vec<CommandSpec>,
    tools: Vec<ToolEntry>,
    collisions: Vec<String>,
    responder: Box<dyn PermissionResponder + Send>,
    negotiated: Option<String>,
    buf: Vec<u8>,
}

impl McpServer {
    /// Build the tool table from the command table.
    ///
    /// The caller must seed its [`CommandService`] from the *same* slice, or a tool will
    /// be advertised that dispatch does not know — which surfaces as an `isError`
    /// result rather than anything worse, but is still a bug in the wiring.
    pub fn new(specs: &[CommandSpec], responder: Box<dyn PermissionResponder + Send>) -> Self {
        let mut server = Self {
            server_name: DEFAULT_SERVER_NAME.to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            permission_tool: DEFAULT_PERMISSION_TOOL.to_string(),
            specs: Vec::new(),
            tools: Vec::new(),
            collisions: Vec::new(),
            responder,
            negotiated: None,
            buf: Vec::new(),
        };
        server.set_tools(specs);
        server
    }

    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = name.into();
        self
    }

    pub fn with_server_version(mut self, version: impl Into<String>) -> Self {
        self.server_version = version.into();
        self
    }

    /// Rename the permission handler. Whatever it is called, the flag spells it
    /// `mcp__<server_name>__<this>`.
    pub fn with_permission_tool(mut self, name: impl Into<String>) -> Self {
        self.permission_tool = name.into();
        // The permission tool wins a name fight, so the whole table is re-resolved
        // against the new name — a command that collided under the old one may be
        // servable now, and vice versa. Re-resolving from the retained specs rather
        // than from `tools` is what makes that "vice versa" true; the served set has
        // already forgotten the losers.
        let specs = std::mem::take(&mut self.specs);
        self.set_tools(&specs);
        self
    }

    /// Replace the tool table wholesale — re-seeding after the catalog changed.
    pub fn set_tools(&mut self, specs: &[CommandSpec]) {
        self.specs = specs.to_vec();
        self.tools.clear();
        self.collisions.clear();
        for spec in specs {
            self.push_tool(ToolEntry::from_spec(spec));
        }
    }

    fn push_tool(&mut self, entry: ToolEntry) {
        let taken = entry.tool_name == self.permission_tool
            || self.tools.iter().any(|t| t.tool_name == entry.tool_name);
        if taken {
            self.collisions.push(entry.command_name);
        } else {
            self.tools.push(entry);
        }
    }

    pub fn tools(&self) -> &[ToolEntry] {
        &self.tools
    }

    /// Commands whose sanitised tool name was already taken, and which are therefore
    /// **not served**. Empty is the normal case; anything here is a naming bug the
    /// caller must resolve, and the console should surface it rather than run half a
    /// catalog silently.
    pub fn name_collisions(&self) -> &[String] {
        &self.collisions
    }

    pub fn permission_tool(&self) -> &str {
        &self.permission_tool
    }

    /// The exact string for `--permission-prompt-tool`.
    pub fn permission_tool_flag_value(&self) -> String {
        format!("mcp__{}__{}", self.server_name, self.permission_tool)
    }

    /// Every served capability tool, spelled the way the **client** names it — the same
    /// `mcp__<server>__<tool>` form as [`Self::permission_tool_flag_value`], and the form
    /// that appears in `system/init`'s `tools` array.
    ///
    /// One formatting site for both, so an audit of one against the other cannot be
    /// comparing two spellings of the same thing.
    pub fn namespaced_tool_names(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|t| format!("mcp__{}__{}", self.server_name, t.tool_name))
            .collect()
    }

    /// **The security property, re-measured against this server** —
    /// `doc/console_approval_protocol.md` §7 and §9 point 4.
    ///
    /// 🚨 §7 measured that Claude Code **withholds the `--permission-prompt-tool` handler
    /// from the model's own tool list**, so the model cannot call it and hand itself
    /// `{"behavior":"allow"}`. §9 point 4 says the guarantee is tied to the flag and must be
    /// re-measured **per server** — and serving real capability tools from the same server
    /// is exactly the change that could disturb it.
    ///
    /// This makes that check something the console performs on itself at every
    /// `system/init`, from the `tools` array the CLI already reports, rather than something
    /// a person remembers to run once. A property nobody re-measures is a property that
    /// quietly stops being true.
    ///
    /// ⚠️ `offered` must be the init event's list **verbatim**. An empty list is not a pass:
    /// it means the session reported nothing, and [`ExposureAudit::reported`] is what says
    /// so — a "handler absent" read off no data at all is the false negative this whole
    /// arrangement exists to avoid.
    pub fn audit_exposure(&self, offered: &[String]) -> ExposureAudit {
        ExposureAudit::of(
            &self.permission_tool_flag_value(),
            &self.namespaced_tool_names(),
            offered,
        )
    }

    /// The protocol revision agreed at `initialize`, once one has been.
    pub fn negotiated_protocol_version(&self) -> Option<&str> {
        self.negotiated.as_deref()
    }

    /// Is this message the one call that can block for as long as a human takes?
    ///
    /// The transport asks before it answers, because a permission call needs a response it
    /// can *write to while it waits* — both to hold the deadline open and, more basically,
    /// to notice that the client has gone. A capability tool returns at once and needs
    /// neither.
    pub fn is_permission_call(&self, message: &Value) -> bool {
        message.get("method").and_then(Value::as_str) == Some("tools/call")
            && message
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                == Some(self.permission_tool.as_str())
    }

    /// The `progressToken` of a permission `tools/call`, if this message is one.
    ///
    /// 🚨 **This is the question a transport must ask before it answers.** A permission
    /// call is the only message here that can block for minutes, and the token is the only
    /// handle the client gives us for saying "still working" — measured in the `_meta`
    /// block the client attaches to every one of them (doc §3):
    ///
    /// ```text
    /// "_meta":{"claudecode/toolUseId":"toolu_…","progressToken":2}
    /// ```
    ///
    /// Deliberately narrow. A `tools/call` for a *capability* tool returns at once and has
    /// nothing to report, and a token on any other method is not ours to answer against —
    /// so this says yes to exactly the one call that needs the deadline held open.
    ///
    /// ⚠️ `None` for a permission call with **no** token is the honest answer, not a
    /// fallback: without one there is nothing to address a notification to, and inventing
    /// a token would be a keep-alive the client discards while its clock runs out. The
    /// transport still streams such a call — see [`Self::is_permission_call`] — because
    /// noticing the client leave does not need a token, only an open connection.
    pub fn permission_progress_token(&self, message: &Value) -> Option<Value> {
        if !self.is_permission_call(message) {
            return None;
        }
        message.get("params")?.get("_meta")?.get("progressToken").cloned()
    }

    // -- message level -----------------------------------------------------

    /// Handle one decoded message. `None` means "no response is owed" — a notification,
    /// or something that is not a request at all.
    ///
    /// Nothing here latches: an error answer leaves the server exactly as it found it,
    /// which is what lets the measured `server/discover` probe be answered `-32601` and
    /// the very next `tools/list` still work.
    pub fn handle(&mut self, message: &Value, dispatch: &mut dyn ToolDispatch) -> Option<Value> {
        self.handle_with(message, dispatch, &mut NoHeartbeat)
    }

    /// [`Self::handle`], plus the keep-alive a blocking permission answer needs.
    ///
    /// Split rather than folded into `handle` because only one caller has a connection to
    /// hold open — [`crate::mcp_http`] — and every other caller, tests included, would
    /// have to pass a [`NoHeartbeat`] it has no use for.
    pub fn handle_with(
        &mut self,
        message: &Value,
        dispatch: &mut dyn ToolDispatch,
        beat: &mut dyn Heartbeat,
    ) -> Option<Value> {
        let object = match message {
            Value::Object(map) => map,
            _ => {
                return Some(error_response(
                    Value::Null,
                    INVALID_REQUEST,
                    "a JSON-RPC message must be a single object; batches are not supported",
                ))
            }
        };

        let is_request = object.contains_key("id");
        let id = object.get("id").cloned().unwrap_or(Value::Null);

        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // A response, or malformed. Never answer a message that is not a request.
            return is_request
                .then(|| error_response(id, INVALID_REQUEST, "missing 'method'"));
        };
        let params = object.get("params").cloned().unwrap_or(Value::Null);

        let outcome = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tool_definitions() })),
            "tools/call" => self.call_tool(&params, dispatch, beat),
            // Notifications we recognise and have nothing to do about. They carry no
            // id, so the value never leaves.
            m if m.starts_with("notifications/") => Ok(json!({})),
            other => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("unknown method '{other}'"),
            )),
        };

        if !is_request {
            return None;
        }
        Some(match outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(err) => error_response(id, err.code, err.message),
        })
    }

    fn tool_definitions(&self) -> Vec<Value> {
        let mut out: Vec<Value> = self.tools.iter().map(ToolEntry::definition).collect();
        out.push(json!({
            "name": self.permission_tool,
            "description":
                "Answer a permission request from Claude Code. Called by the client via \
                 --permission-prompt-tool; not for the model to call directly.",
            "inputSchema": permission_schema(),
        }));
        out
    }

    /// We echo the client's revision when we know it, and answer with our latest when we
    /// do not — the spec's negotiation, and the reason `2026-07-28` is not in the
    /// supported list.
    fn initialize(&mut self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let agreed = match asked {
            Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v.to_string(),
            _ => LATEST_PROTOCOL_VERSION.to_string(),
        };
        self.negotiated = Some(agreed.clone());
        json!({
            "protocolVersion": agreed,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": self.server_name, "version": self.server_version },
        })
    }

    fn call_tool(
        &mut self,
        params: &Value,
        dispatch: &mut dyn ToolDispatch,
        beat: &mut dyn Heartbeat,
    ) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError::new(INVALID_PARAMS, "tools/call requires a string 'name'"));
        };
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

        if name == self.permission_tool {
            return self.answer_permission(&arguments, beat);
        }

        // An unknown *tool* is a protocol fault (the client called something we never
        // listed); an unknown *argument* is a tool fault the model can correct. The two
        // get different shapes on purpose.
        let Some(command) = self
            .tools
            .iter()
            .find(|t| t.tool_name == name)
            .map(|t| t.command_name.clone())
        else {
            return Err(RpcError::new(INVALID_PARAMS, format!("unknown tool '{name}'")));
        };

        Ok(match dispatch.call(&command, arguments) {
            // Always serialised, never unwrapped: one rule beats a special case per
            // result shape, and a model that wants structure can parse it.
            Ok(value) => text_result(&value.to_string(), false),
            Err(message) => text_result(&message, true),
        })
    }

    fn answer_permission(
        &mut self,
        arguments: &Value,
        beat: &mut dyn Heartbeat,
    ) -> Result<Value, RpcError> {
        let Some(tool_name) = arguments.get("tool_name").and_then(Value::as_str) else {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "permission request requires a string 'tool_name'",
            ));
        };
        let request = PermissionRequest {
            tool_name: tool_name.to_string(),
            input: arguments.get("input").cloned().unwrap_or_else(|| json!({})),
            // Liberal on purpose — see `permission_schema`.
            tool_use_id: arguments
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        let decision = self.responder.decide(&request, beat);
        // A deny is a *successful* answer to the question, so `isError` stays false.
        Ok(text_result(&decision.to_wire(), false))
    }

    // -- byte level --------------------------------------------------------

    /// Handle one whole line of input. A line that is not JSON is a `-32700` with a null
    /// id — reported, never swallowed, and the stream continues.
    pub fn handle_line(&mut self, line: &[u8], dispatch: &mut dyn ToolDispatch) -> Option<Value> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        match serde_json::from_slice::<Value>(line) {
            Ok(message) => self.handle(&message, dispatch),
            Err(err) => Some(error_response(Value::Null, PARSE_ERROR, err.to_string())),
        }
    }

    /// Feed a chunk of bytes; get back one response per *complete* line that owed one.
    ///
    /// **The framing contract**: messages are newline-delimited JSON, and a message is
    /// only processed once its terminating `\n` has arrived. A chunk that ends
    /// mid-message is buffered — pipe reads split wherever the OS pleases, and a decoder
    /// that assumed one read is one message would fail intermittently under load.
    /// Splitting on `\n` byte-wise is safe because `\n` cannot occur inside a UTF-8
    /// multi-byte sequence.
    pub fn push(&mut self, chunk: &[u8], dispatch: &mut dyn ToolDispatch) -> Vec<Value> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut start = 0usize;
        while let Some(offset) = self.buf[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + offset;
            let line = self.buf[start..end].to_vec();
            if let Some(response) = self.handle_line(&line, dispatch) {
                out.push(response);
            }
            start = end + 1;
        }
        if start > 0 {
            self.buf.drain(..start);
        }
        out
    }

    /// Drive the server from a pair of byte streams until the reader ends.
    ///
    /// ⚠️ **`writer` carries JSON-RPC and nothing else** — one compact object per line,
    /// flushed per message. Anything else on that stream corrupts it. Log elsewhere.
    pub fn serve<R: Read, W: Write>(
        &mut self,
        mut reader: R,
        mut writer: W,
        dispatch: &mut dyn ToolDispatch,
    ) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                return Ok(());
            }
            for response in self.push(&chunk[..read], dispatch) {
                writer.write_all(response.to_string().as_bytes())?;
                writer.write_all(b"\n")?;
                writer.flush()?;
            }
        }
    }
}

/// Ordinary MCP text content. Tool *failures* ride here with `isError`, so the model can
/// read the reason and correct itself; protocol faults are JSON-RPC errors instead.
fn text_result(text: &str, is_error: bool) -> Value {
    json!({
        "content": [ { "type": "text", "text": text } ],
        "isError": is_error,
    })
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{ArgSpec, CommandError, MockTarget, TargetKind};
    use crate::session::SessionLog;
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    // -- fixtures ---------------------------------------------------------

    /// A fixture catalog exercising every [`ArgKind`], plus a dotted name and a
    /// zero-argument command.
    fn fixture_specs() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                name: "viewport.set".into(),
                doc: "Set a viewport parameter".into(),
                target: TargetKind::Viewport,
                args: vec![
                    ArgSpec {
                        name: "gain".into(),
                        kind: ArgKind::Float { min: 0.0, max: 1.0 },
                        required: true,
                    },
                    ArgSpec {
                        name: "mode".into(),
                        kind: ArgKind::Choice(vec!["calm".into(), "storm".into()]),
                        required: false,
                    },
                    ArgSpec { name: "count".into(), kind: ArgKind::Int, required: false },
                    ArgSpec { name: "on".into(), kind: ArgKind::Bool, required: false },
                    ArgSpec { name: "label".into(), kind: ArgKind::Text, required: false },
                ],
            },
            CommandSpec {
                name: "shell.echo".into(),
                doc: "Return the arguments unchanged".into(),
                target: TargetKind::Project,
                args: Vec::new(),
            },
        ]
    }

    /// A dispatch double: records every call, answers from a script.
    #[derive(Clone, Default)]
    struct RecordingDispatch {
        state: Rc<RefCell<(Vec<(String, Value)>, Vec<Result<Value, String>>)>>,
    }

    impl RecordingDispatch {
        fn new() -> Self {
            Self::default()
        }
        fn calls(&self) -> Vec<(String, Value)> {
            self.state.borrow().0.clone()
        }
        fn push_result(&self, result: Result<Value, String>) {
            self.state.borrow_mut().1.push(result);
        }
    }

    impl ToolDispatch for RecordingDispatch {
        fn call(&mut self, command: &str, args: Value) -> Result<Value, String> {
            let mut state = self.state.borrow_mut();
            state.0.push((command.to_string(), args));
            if state.1.is_empty() {
                Ok(Value::Null)
            } else {
                state.1.remove(0)
            }
        }
    }

    /// A dispatch that must never be reached.
    struct NeverDispatch;
    impl ToolDispatch for NeverDispatch {
        fn call(&mut self, command: &str, _args: Value) -> Result<Value, String> {
            panic!("dispatch reached for '{command}'");
        }
    }

    fn deny_all() -> Box<dyn PermissionResponder + Send> {
        Box::new(|_: &PermissionRequest| PermissionDecision::deny("no"))
    }

    fn server() -> McpServer {
        McpServer::new(&fixture_specs(), deny_all())
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    }

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("organon-shell-mcp-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// The single text block out of a tool result, and whether it was flagged an error.
    fn tool_text(response: &Value) -> (String, bool) {
        let result = &response["result"];
        let text = result["content"][0]["text"].as_str().expect("text content").to_string();
        (text, result["isError"].as_bool().expect("isError"))
    }

    // -- handshake ---------------------------------------------------------

    #[test]
    fn initialize_echoes_a_supported_protocol_version() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let response = server
            .handle(
                &request(1, "initialize", json!({ "protocolVersion": "2025-11-25" })),
                &mut dispatch,
            )
            .expect("a request is owed a response");

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(response["result"]["serverInfo"]["name"], DEFAULT_SERVER_NAME);
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert_eq!(server.negotiated_protocol_version(), Some("2025-11-25"));
    }

    #[test]
    fn initialize_falls_back_to_our_latest_for_an_unknown_revision() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        // The `server/discover` probe's revision — never negotiated, only probed with.
        let response = server
            .handle(
                &request(1, "initialize", json!({ "protocolVersion": "2026-07-28" })),
                &mut dispatch,
            )
            .unwrap();
        assert_eq!(response["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    /// The measured startup sequence (doc, "Implementation notes"): a `server/discover`
    /// probe at `2026-07-28` must earn `-32601` and leave the server usable, and the
    /// respawned second process must handshake from cold.
    #[test]
    fn the_server_discover_probe_is_method_not_found_and_does_not_poison_the_stream() {
        let mut first = server();
        let mut dispatch = NeverDispatch;
        let probe = first
            .handle(
                &request(0, "server/discover", json!({ "protocolVersion": "2026-07-28" })),
                &mut dispatch,
            )
            .unwrap();
        assert_eq!(probe["error"]["code"], METHOD_NOT_FOUND);
        assert!(probe["error"]["message"].as_str().unwrap().contains("server/discover"));
        assert_eq!(first.negotiated_protocol_version(), None, "a probe negotiates nothing");

        // Same process, next message: still fully functional.
        let listed = first.handle(&request(1, "tools/list", json!({})), &mut dispatch).unwrap();
        assert!(listed["result"]["tools"].as_array().unwrap().len() >= 2);

        // The respawn — a cold server, identical behaviour.
        let mut second = McpServer::new(&fixture_specs(), deny_all());
        let init = second
            .handle(
                &request(1, "initialize", json!({ "protocolVersion": "2025-11-25" })),
                &mut dispatch,
            )
            .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn notifications_are_never_answered() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        assert!(server
            .handle(
                &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                &mut dispatch
            )
            .is_none());
        // Not even an unknown one — a notification carries no id to answer.
        assert!(server
            .handle(&json!({ "jsonrpc": "2.0", "method": "nonsense/whatever" }), &mut dispatch)
            .is_none());
        // And a response object is not a request either.
        assert!(server
            .handle(&json!({ "jsonrpc": "2.0", "result": {} }), &mut dispatch)
            .is_none());
    }

    #[test]
    fn a_batch_array_is_an_invalid_request_not_a_panic() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let response = server
            .handle(&json!([request(1, "tools/list", json!({}))]), &mut dispatch)
            .unwrap();
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
        assert_eq!(response["id"], Value::Null);
    }

    // -- the generated tool table -----------------------------------------

    #[test]
    fn tools_list_mirrors_the_command_table_exactly() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let response = server.handle(&request(7, "tools/list", json!({})), &mut dispatch).unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["viewport_set", "shell_echo", DEFAULT_PERMISSION_TOOL]);

        let set = &tools[0];
        assert_eq!(set["description"], "Set a viewport parameter");
        assert_eq!(
            set["inputSchema"],
            json!({
                "type": "object",
                "properties": {
                    "gain":  { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                    "mode":  { "type": "string", "enum": ["calm", "storm"] },
                    "count": { "type": "integer" },
                    "on":    { "type": "boolean" },
                    "label": { "type": "string" }
                },
                "required": ["gain"],
                "additionalProperties": true
            }),
            "every ArgKind, generated — never a hand-written second copy"
        );

        // A zero-argument command still gets a well-formed object schema, and no
        // `required` key at all rather than an empty array.
        let echo = &tools[1];
        assert_eq!(
            echo["inputSchema"],
            json!({ "type": "object", "properties": {}, "additionalProperties": true })
        );
    }

    /// Dotted command names are illegal as MCP tool names (`^[a-zA-Z0-9_-]{1,128}$`), so
    /// the table is sanitised — and the mapping back is kept, not recomputed.
    #[test]
    fn dotted_command_names_become_legal_tool_names_and_map_back() {
        let server = server();
        let entry = &server.tools()[0];
        assert_eq!(entry.command_name, "viewport.set");
        assert_eq!(entry.tool_name, "viewport_set");
        assert_eq!(tool_name_for("a b/c.d:e"), "a_b_c_d_e");
        assert_eq!(tool_name_for("already-legal_9"), "already-legal_9");
        assert_eq!(tool_name_for(""), "_", "never an empty tool name");
        assert_eq!(tool_name_for(&"x".repeat(200)).len(), 128, "clamped to the grammar");
    }

    /// Sanitising is many-to-one, so it can collide. A collision must be visible, never
    /// a silent shadow — the console can then rename before shipping a half-catalog.
    #[test]
    fn colliding_tool_names_are_reported_rather_than_shadowing() {
        let specs = vec![
            CommandSpec {
                name: "a.b".into(),
                doc: "first".into(),
                target: TargetKind::Project,
                args: Vec::new(),
            },
            CommandSpec {
                name: "a/b".into(),
                doc: "second".into(),
                target: TargetKind::Project,
                args: Vec::new(),
            },
            // A command that would impersonate the permission handler loses too.
            CommandSpec {
                name: DEFAULT_PERMISSION_TOOL.into(),
                doc: "impostor".into(),
                target: TargetKind::Project,
                args: Vec::new(),
            },
        ];
        let server = McpServer::new(&specs, deny_all());
        assert_eq!(server.tools().len(), 1);
        assert_eq!(server.tools()[0].command_name, "a.b");
        assert_eq!(server.name_collisions(), ["a/b", DEFAULT_PERMISSION_TOOL]);

        // Renaming the handler frees the name it was holding — the table re-resolves
        // from the specs, not from the set that already forgot the losers.
        let renamed = server.with_permission_tool("console_may_i");
        let served: Vec<&str> =
            renamed.tools().iter().map(|t| t.command_name.as_str()).collect();
        assert_eq!(served, ["a.b", DEFAULT_PERMISSION_TOOL]);
        assert_eq!(renamed.name_collisions(), ["a/b"]);
    }

    /// ⚠️ `serde_json` renders a non-finite float as `null`. An unbounded axis must
    /// therefore carry no bound at all — `"minimum": null` is not a schema.
    #[test]
    fn a_non_finite_bound_is_omitted_rather_than_serialised_as_null() {
        let spec = CommandSpec {
            name: "runtime.open".into(),
            doc: "unbounded".into(),
            target: TargetKind::Runtime,
            args: vec![ArgSpec {
                name: "amount".into(),
                kind: ArgKind::Float { min: f64::NEG_INFINITY, max: f64::INFINITY },
                required: true,
            }],
        };
        let schema = input_schema(&spec);
        assert_eq!(schema["properties"]["amount"], json!({ "type": "number" }));
        assert!(!schema.to_string().contains("null"));
    }

    #[test]
    fn the_permission_tool_is_served_and_its_flag_value_is_spelled_out() {
        let server = server().with_server_name("organon").with_permission_tool("approve_tool");
        assert_eq!(server.permission_tool_flag_value(), "mcp__organon__approve_tool");
        let mut server = server;
        let mut dispatch = NeverDispatch;
        let response = server.handle(&request(1, "tools/list", json!({})), &mut dispatch).unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        let handler = tools.last().unwrap();
        assert_eq!(handler["name"], "approve_tool");
        assert_eq!(
            handler["inputSchema"]["required"],
            json!(["tool_name", "input", "tool_use_id"])
        );
    }

    /// **The drift guard, as a test rather than as a promise.** A served tool's schema is a
    /// function of its [`CommandSpec`] and of nothing else, so a spec that gains an argument
    /// gains it in the tool the same instant. This tree has already paid for the other
    /// arrangement — three hand-maintained range tables, nine of forty-five ids silently
    /// wrong, and the wrong bounds published.
    #[test]
    fn every_served_tool_carries_the_schema_its_spec_generates() {
        let specs = fixture_specs();
        let mut server = McpServer::new(&specs, deny_all());
        let mut dispatch = NeverDispatch;
        let response = server.handle(&request(1, "tools/list", json!({})), &mut dispatch).unwrap();
        let listed = response["result"]["tools"].as_array().unwrap().clone();

        for spec in &specs {
            let name = tool_name_for(&spec.name);
            let served = listed
                .iter()
                .find(|t| t["name"] == json!(name))
                .unwrap_or_else(|| panic!("'{}' must be served as '{name}'", spec.name));
            assert_eq!(
                served["inputSchema"],
                input_schema(spec),
                "'{}' is served a schema its spec did not generate",
                spec.name
            );
            assert_eq!(served["description"], json!(spec.doc), "the doc is the description");
        }
        assert_eq!(
            listed.len(),
            specs.len() + 1,
            "every spec, plus the handler, and nothing hand-added"
        );
    }

    // -- the exposure audit (§7, re-measured per server) -------------------

    /// 🚨 **The security property, and what the console checks about itself every init.**
    /// §7 measured that Claude Code withholds the `--permission-prompt-tool` handler from
    /// the model's own tool list; §9 point 4 says re-measure it per server. Serving real
    /// capability tools is exactly the change that could disturb it, so the check has to
    /// distinguish three states, not two.
    #[test]
    fn the_audit_separates_a_withheld_handler_from_a_reachable_one_and_from_no_data() {
        let mut server = server().with_server_name("organon");
        let handler = server.permission_tool_flag_value();
        let served = server.namespaced_tool_names();
        assert_eq!(served, ["mcp__organon__viewport_set", "mcp__organon__shell_echo"]);
        assert!(!served.contains(&handler), "the handler is not one of the capability tools");

        // ⚠️ **Two different lists, and the distinction is the whole property.** The handler
        // IS in our `tools/list` — it has to be, or the client could not call it — and must
        // NOT be in the tools the *model* is offered. This audit reads the second.
        let mut dispatch = NeverDispatch;
        let listed = server.handle(&request(1, "tools/list", json!({})), &mut dispatch).unwrap();
        let names: Vec<String> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&DEFAULT_PERMISSION_TOOL.to_string()), "the client can call it");

        // The measured shape: our verbs offered, the handler absent.
        let healthy = server.audit_exposure(&[
            "Bash".to_string(),
            "Read".to_string(),
            served[0].clone(),
            served[1].clone(),
        ]);
        assert!(!healthy.handler_offered);
        assert_eq!(healthy.capabilities_offered, served);
        assert!(healthy.capabilities_withheld.is_empty());
        assert!(healthy.summary().contains("handler withheld"));

        // The failure: the handler in the model's own list. It must not be able to read as
        // the healthy line.
        let breached = server.audit_exposure(&["Bash".to_string(), handler.clone()]);
        assert!(breached.handler_offered);
        assert!(breached.summary().starts_with("🚨"));
        assert!(!breached.summary().contains("handler withheld"));

        // ⚠️ And the third state: nothing reported proves nothing. A pass read off an empty
        // list is the false negative this exists to avoid.
        let blind = server.audit_exposure(&[]);
        assert!(!blind.handler_offered);
        assert!(!blind.reported_anything());
        assert!(blind.summary().contains("could not be checked"));
    }

    /// A served tool the model cannot see is reported rather than assumed away — MCP tools
    /// arrive deferred, so this is information, not a fault.
    #[test]
    fn a_served_tool_missing_from_the_model_s_list_is_named() {
        let server = server().with_server_name("organon");
        let audit = server.audit_exposure(&["Bash".to_string(), "mcp__organon__shell_echo".into()]);
        assert_eq!(audit.capabilities_offered, ["mcp__organon__shell_echo"]);
        assert_eq!(audit.capabilities_withheld, ["mcp__organon__viewport_set"]);
        assert!(audit.summary().contains("1 of 2 console tools visible"));
    }

    // -- tools/call --------------------------------------------------------

    #[test]
    fn a_valid_call_dispatches_under_the_command_name_and_returns_its_result() {
        let mut server = server();
        let recorder = RecordingDispatch::new();
        recorder.push_result(Ok(json!({ "applied": true })));
        let mut dispatch = recorder.clone();

        let response = server
            .handle(
                &request(
                    3,
                    "tools/call",
                    json!({ "name": "viewport_set", "arguments": { "gain": 0.5 } }),
                ),
                &mut dispatch,
            )
            .unwrap();

        assert_eq!(
            recorder.calls(),
            vec![("viewport.set".to_string(), json!({ "gain": 0.5 }))],
            "the MCP name is resolved back to the command name"
        );
        let (text, is_error) = tool_text(&response);
        assert!(!is_error);
        assert_eq!(text, r#"{"applied":true}"#);
    }

    #[test]
    fn an_unknown_tool_is_invalid_params_and_never_dispatches() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let response = server
            .handle(&request(4, "tools/call", json!({ "name": "nope" })), &mut dispatch)
            .unwrap();
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
        assert!(response["error"]["message"].as_str().unwrap().contains("nope"));
    }

    /// The whole point of routing through [`CommandService`]: the arguments are checked
    /// against the same [`CommandSpec`] the schema was generated from, the target is
    /// never reached, and the failure comes back as an `isError` result the model can
    /// read and correct — not a JSON-RPC error it cannot.
    #[test]
    fn invalid_arguments_are_an_error_result_and_never_reach_the_target() {
        let root = temp_root("invalid-args");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        let spec = fixture_specs().remove(0);
        service.register_spec(spec.clone());
        let mock = MockTarget::new();
        service.register_target(TargetKind::Viewport, Box::new(mock.handle()));

        let mut server = McpServer::new(&[spec], deny_all());
        let mut dispatch = ServiceDispatch::new(&mut service, Issuer::PrimaryAgent);

        let response = server
            .handle(
                &request(
                    5,
                    "tools/call",
                    json!({ "name": "viewport_set", "arguments": { "gain": 9.0 } }),
                ),
                &mut dispatch,
            )
            .unwrap();

        let (text, is_error) = tool_text(&response);
        assert!(is_error, "a rejected call is a tool error, not a protocol error");
        assert_eq!(text, "viewport.set: argument 'gain': out of range: 9 not in 0..1");
        assert!(mock.calls().is_empty(), "rejected args never reach the target");
    }

    #[test]
    fn a_valid_call_through_the_real_service_reaches_the_target() {
        let root = temp_root("service-dispatch");
        let mut log = SessionLog::open(&root, "s1").unwrap();
        let mut service = CommandService::new(&mut log);
        let spec = fixture_specs().remove(0);
        service.register_spec(spec.clone());
        let mock = MockTarget::new();
        mock.push_result(Ok(json!("done")));
        service.register_target(TargetKind::Viewport, Box::new(mock.handle()));

        let mut server = McpServer::new(&[spec], deny_all());
        let mut dispatch = ServiceDispatch::new(&mut service, Issuer::Worker("claude-code".into()));
        let response = server
            .handle(
                &request(
                    6,
                    "tools/call",
                    json!({ "name": "viewport_set", "arguments": { "gain": 0.25, "mode": "calm" } }),
                ),
                &mut dispatch,
            )
            .unwrap();

        let (text, is_error) = tool_text(&response);
        assert!(!is_error);
        assert_eq!(text, r#""done""#);
        assert_eq!(mock.calls().len(), 1);
        assert_eq!(mock.calls()[0].0, "viewport.set");
    }

    #[test]
    fn a_target_failure_comes_back_as_an_error_result() {
        let mut server = server();
        let recorder = RecordingDispatch::new();
        recorder.push_result(Err(CommandError::Execution {
            command: "viewport.set".into(),
            message: "runtime went away".into(),
        }
        .to_string()));
        let mut dispatch = recorder.clone();
        let response = server
            .handle(
                &request(
                    8,
                    "tools/call",
                    json!({ "name": "viewport_set", "arguments": { "gain": 0.5 } }),
                ),
                &mut dispatch,
            )
            .unwrap();
        let (text, is_error) = tool_text(&response);
        assert!(is_error);
        assert_eq!(text, "viewport.set: runtime went away");
    }

    // -- the permission handler -------------------------------------------

    /// Byte-exact against `doc/console_approval_protocol.md` §4's quoted allow payload.
    #[test]
    fn allow_payload_is_byte_exact() {
        let request = PermissionRequest {
            tool_name: "mcp__probe__echo_probe".into(),
            input: json!({ "text": "ALPHA" }),
            tool_use_id: "toolu_011k3yjKzmcedxXdEGWcwhpg".into(),
        };
        assert_eq!(
            PermissionDecision::allow_unchanged(&request).to_wire(),
            r#"{"behavior":"allow","updatedInput":{"text":"ALPHA"}}"#
        );
    }

    /// Byte-exact against §4's quoted deny payload.
    #[test]
    fn deny_payload_is_byte_exact() {
        assert_eq!(
            PermissionDecision::deny("probe handler denied this call on purpose").to_wire(),
            r#"{"behavior":"deny","message":"probe handler denied this call on purpose"}"#
        );
    }

    /// The three flat arguments of §3 reach the hook intact — `tool_use_id` above all,
    /// since it is what attaches a card to the tool element already in the transcript.
    #[test]
    fn the_hook_sees_the_whole_request_including_the_tool_use_id() {
        // `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>` because the responder box is
        // `Send` — see the module doc: the console's hook lives on a serve thread.
        let seen: Arc<Mutex<Vec<PermissionRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut server = McpServer::new(
            &fixture_specs(),
            Box::new(move |req: &PermissionRequest| {
                sink.lock().unwrap().push(req.clone());
                PermissionDecision::allow_unchanged(req)
            }),
        );
        let mut dispatch = NeverDispatch;

        let response = server
            .handle(
                &request(
                    2,
                    "tools/call",
                    json!({
                        "name": DEFAULT_PERMISSION_TOOL,
                        "arguments": {
                            "tool_name": "Bash",
                            "input": { "command": "env | head -2" },
                            "tool_use_id": "toolu_01SwDhC1uZLFQ6NwLuPbxvb5"
                        }
                    }),
                ),
                &mut dispatch,
            )
            .unwrap();

        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "Bash", "the handler answers for Bash too");
        assert_eq!(calls[0].input, json!({ "command": "env | head -2" }));
        assert_eq!(calls[0].tool_use_id, "toolu_01SwDhC1uZLFQ6NwLuPbxvb5");

        let (text, is_error) = tool_text(&response);
        assert!(!is_error, "an answered question is a successful call");
        assert_eq!(text, r#"{"behavior":"allow","updatedInput":{"command":"env | head -2"}}"#);
    }

    /// `updatedInput` is not ceremony — it is the seam for "allow, but not that path".
    #[test]
    fn updated_input_round_trips_and_may_differ_from_the_input() {
        let mut server = McpServer::new(
            &fixture_specs(),
            Box::new(|req: &PermissionRequest| {
                let mut input = req.input.clone();
                input["command"] = json!("env | head -1");
                PermissionDecision::allow_with(input)
            }),
        );
        let mut dispatch = NeverDispatch;
        let response = server
            .handle(
                &request(
                    3,
                    "tools/call",
                    json!({
                        "name": DEFAULT_PERMISSION_TOOL,
                        "arguments": {
                            "tool_name": "Bash",
                            "input": { "command": "env | head -2" },
                            "tool_use_id": "toolu_x"
                        }
                    }),
                ),
                &mut dispatch,
            )
            .unwrap();
        let (text, _) = tool_text(&response);
        assert_eq!(text, r#"{"behavior":"allow","updatedInput":{"command":"env | head -1"}}"#);
    }

    /// The runtime handler is more liberal than its own schema: a missing `tool_use_id`
    /// yields an empty string, because an uncorrelated card still beats an unanswerable
    /// approval. A missing `tool_name` is genuinely unanswerable, and is refused.
    #[test]
    fn the_handler_tolerates_a_missing_tool_use_id_but_not_a_missing_tool_name() {
        let mut server = McpServer::new(
            &fixture_specs(),
            Box::new(PermissionDecision::allow_unchanged as fn(&PermissionRequest) -> _),
        );
        let mut dispatch = NeverDispatch;

        let ok = server
            .handle(
                &request(
                    1,
                    "tools/call",
                    json!({
                        "name": DEFAULT_PERMISSION_TOOL,
                        "arguments": { "tool_name": "Bash", "input": {} }
                    }),
                ),
                &mut dispatch,
            )
            .unwrap();
        assert_eq!(tool_text(&ok).0, r#"{"behavior":"allow","updatedInput":{}}"#);

        let bad = server
            .handle(
                &request(
                    2,
                    "tools/call",
                    json!({ "name": DEFAULT_PERMISSION_TOOL, "arguments": { "input": {} } }),
                ),
                &mut dispatch,
            )
            .unwrap();
        assert_eq!(bad["error"]["code"], INVALID_PARAMS);
    }

    /// The doc measured three identical calls producing three separate handler requests:
    /// there is no upstream persistence, so the console's memory is the only memory.
    /// This pins that the server itself never caches a verdict.
    #[test]
    fn every_call_asks_the_hook_again() {
        let count = Arc::new(Mutex::new(0usize));
        let tally = count.clone();
        let mut server = McpServer::new(
            &fixture_specs(),
            Box::new(move |req: &PermissionRequest| {
                *tally.lock().unwrap() += 1;
                PermissionDecision::allow_unchanged(req)
            }),
        );
        let mut dispatch = NeverDispatch;
        let message = request(
            1,
            "tools/call",
            json!({
                "name": DEFAULT_PERMISSION_TOOL,
                "arguments": { "tool_name": "Bash", "input": { "command": "ls" }, "tool_use_id": "t" }
            }),
        );
        for _ in 0..3 {
            assert!(server.handle(&message, &mut dispatch).is_some());
        }
        assert_eq!(*count.lock().unwrap(), 3);
    }

    // -- framing -----------------------------------------------------------

    #[test]
    fn a_message_split_across_reads_is_buffered_until_its_newline() {
        let mut server = server();
        let mut dispatch = NeverDispatch;

        assert!(server.push(br#"{"jsonrpc":"2.0","id":1,"meth"#, &mut dispatch).is_empty());
        assert!(server.push(br#"od":"ping","params":{}}"#, &mut dispatch).is_empty());
        let responses = server.push(b"\n", &mut dispatch);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], 1);
        assert_eq!(responses[0]["result"], json!({}));

        // Two messages plus a partial third in one read: two answers, the rest held.
        let responses = server.push(
            b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":4,",
            &mut dispatch,
        );
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["id"], 2);
        assert_eq!(responses[1]["id"], 3);
    }

    #[test]
    fn malformed_json_is_a_parse_error_and_the_stream_continues() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let responses = server.push(b"not json at all\n\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n", &mut dispatch);
        assert_eq!(responses.len(), 2, "the blank line is dropped, not answered");
        assert_eq!(responses[0]["error"]["code"], PARSE_ERROR);
        assert_eq!(responses[0]["id"], Value::Null);
        assert_eq!(responses[1]["id"], 9);
    }

    /// The pipe-driven entry point, over in-memory streams: JSON-RPC out and nothing
    /// else, one compact object per line.
    #[test]
    fn serve_writes_one_json_rpc_line_per_answered_message() {
        let mut server = server();
        let mut dispatch = NeverDispatch;
        let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
                      {\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n"
            .to_vec();
        let mut output: Vec<u8> = Vec::new();
        server.serve(&input[..], &mut output, &mut dispatch).unwrap();

        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "the notification produced no output at all");
        assert_eq!(
            lines[0],
            r#"{"id":1,"jsonrpc":"2.0","result":{}}"#,
            "compact JSON-RPC, nothing else on the stream"
        );
    }
}

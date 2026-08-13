//! The seam: Claude Code's decoded events → the transcript's own events
//! (Console Spike §5.9.3).
//!
//! [`crate::agent_event`] decodes the wire. [`crate::conversation`] folds a transcript.
//! Neither knows about the other, deliberately — "two agents cannot own one type" — so
//! **this module is the only place in the tree that knows both**, and it is the only
//! place a second harness (Pi, §5.9.1) would have to be written against.
//!
//! It is a pure function of the event plus a little carried state, so it is tested
//! headless against the committed captures in `../fixtures/`: no process, no window, no
//! clock. The rules below are not style. Each comes from something measured in a real
//! capture, and getting any of them wrong produces a view that looks nearly right.
//!
//! # 1. An `assistant` line is ONE content block, so the key is per block
//!
//! 🚨 The trap this module exists to close. Three consecutive `assistant` lines in
//! `claude_stream_two_tools.jsonl` share one message id — prose, then tool call #1, then
//! tool call #2. [`crate::conversation::MessageId`] is unique **per rendered text block**
//! and same-id blocks *replace* each other, so passing `message_id` straight through
//! would let the tool call overwrite the prose and **silently lose the assistant's
//! text**.
//!
//! The key is therefore `"{message_id}#{ordinal}"`, where the ordinal counts content
//! blocks within that message. Two independent sources have to agree on it:
//!
//! * the **settled** path counts blocks as `assistant` lines deliver them, in order;
//! * the **streaming** path takes the ordinal straight from `BlockDelta { index }`.
//!
//! They agree because the CLI emits one `assistant` line per block, in block order —
//! verified in the capture, where blocks 0/1/2 of `msg_…0001` arrive as three lines in
//! that order, and the next message restarts at 0. That is why every block consumes an
//! ordinal even when this module renders nothing for it (a `thinking` block, an unknown
//! block kind): skipping one would shift every later block's key by one and detach the
//! deltas from the text they belong to.
//!
//! # 2. The human turn arrives on the stream; nothing is inserted locally
//!
//! `--replay-user-messages` echoes injected input back as an ordinary `user` line
//! flagged `isReplay`. The composer writes to stdin and renders nothing; the transcript
//! renders only what returns. That is what makes ordering free instead of a
//! splice-and-hope — and it is why this module maps replayed and genuine user text
//! identically. There is nothing to tell apart.
//!
//! # 3. `system/init` recurs; only the first establishes identity
//!
//! A second `init` arrived mid-stream in the live-session capture — same `session_id`,
//! different field count — immediately before turn two. Only the first is mapped;
//! later ones produce nothing, because
//! [`Transcript::apply`](crate::conversation::Transcript::apply) would open a fresh turn
//! for them and the human input that follows opens one anyway.
//!
//! # 4. `result` ends a TURN, not the stream
//!
//! It maps to [`RunFinished`](crate::conversation::AgentEvent::RunFinished), which
//! closes nothing — and its `result` text is deliberately **dropped**, because it is the
//! same prose the `assistant` lines already delivered. Rendering both would double every
//! final answer.
//!
//! # 5. Subagent-scoped events are dropped in milestone 1
//!
//! The decoder distinguishes them ([`AgentScope::Subagent`]). Rendered naively they
//! appear as free-floating turns belonging to nobody; they belong *inside* the tool card
//! that spawned them, which is milestone 2. Dropping is a choice with a cost, so it is
//! counted ([`MapStats::subagent_dropped`]) rather than silent.
//!
//! # 6. Some lines carry facts about the session rather than content for the flow
//!
//! `system/init` says which model, which cwd, which permission mode; `result` says what
//! the turn cost; `post_turn_summary` and `rate_limit_event` say what the session's
//! standing is. None of that is an *element* — it is not something the transcript can
//! hold, because the transcript is an ordered list of what was said. So it is
//! accumulated here in [`SessionFacts`] and read by the status strip, exactly the way
//! [`MapStats`] is accumulated and read by the diagnostics.
//!
//! Each field has one retention rule, and the rule is the whole of the correctness:
//! **first-init-wins** for identity (rule 3 — a later init must not overwrite),
//! **latest-wins** for everything that describes the most recent turn. Nothing is
//! summed. See [`SessionFacts`] for what is deliberately *not* carried.
//!
//! # What is not mapped, and why that is not a gap
//!
//! `Notice` (including `post_turn_summary`), `RateLimit`, `tool_use_result`, thinking
//! blocks, and approvals are all held for milestone 2 by §5.9.3's closing note. They are
//! counted in [`MapStats::unmapped`], so "the view showed nothing" and "nothing arrived"
//! stay distinguishable. Reading a *fact* off a notice does not make it mapped: nothing
//! is rendered into the flow for it, so it stays counted exactly as before.

use std::collections::HashMap;

use crate::agent_event::{
    AgentEvent, ContentBlock, Delta, EventKind, Notice, RateLimit, SessionStart, StreamEvent,
    TurnResult, Usage,
};
use crate::conversation as cv;

/// What the mapper chose not to render, counted. Nothing here is an error; all of it is
/// the kind of thing that is invisible until someone asks why a card never appeared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapStats {
    /// Decoded lines seen, subagent lines included.
    pub events: u64,
    /// Dropped for [`AgentScope::Subagent`](crate::agent_event::AgentScope::Subagent).
    pub subagent_dropped: u64,
    /// A `system/init` after the first (rule 3).
    pub repeat_session_starts: u64,
    /// Known event kinds this milestone renders nothing for: notices, rate limits,
    /// thinking, unknown blocks, unknown stream events.
    pub unmapped: u64,
    /// An `input_json_delta` whose block index named no open tool call. Never observed;
    /// counted so a stream shape change cannot hide.
    pub orphan_argument_fragments: u64,
}

/// What the session says about itself, as the stream says it (rule 6).
///
/// Everything here is *reported*, never computed: a field is `None` until a line
/// carrying it arrives, and it then holds that line's value verbatim. An empty string on
/// the wire is absence, not a fact, and is stored as `None`.
///
/// # What this deliberately does not carry
///
/// Three numbers a status strip obviously wants are missing, and each is missing because
/// the stream does not honestly carry it:
///
/// * **A context-window percentage.** The denominator appears only inside the
///   unmodelled `modelUsage` block, per model, and the numerator would have to be a
///   running conversation size nothing on the wire reports. Two guesses would make a
///   confident-looking bar that is wrong.
/// * **A quota percentage.** `rate_limit_event` carries a *status* and a reset time —
///   no numerator, no denominator anywhere.
/// * **A session token total.** Only `total_cost_usd` is cumulative on the wire. The
///   sibling `usage` is per turn, and summing it would double-count every cache read.
///   Cost is taken, tokens are not, and the field names say which is which.
///
/// **`num_turns` is also declined**, for a different reason: it counts the turns of that
/// *run* and does not accumulate (it was `1` on both results of the two-turn capture), so
/// showing it as a session turn counter would show `1` forever. The view counts
/// `Transcript::turns()` instead, which it measured itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFacts {
    // -- from `system/init`, FIRST INIT WINS (rule 3) ------------------------
    /// e.g. `claude-opus-5[1m]`. **Verbatim, suffix included** — that suffix is part of
    /// what the CLI reported, and trimming it would be editorialising a measurement.
    pub model: Option<String>,
    /// The directory the agent operates in.
    pub cwd: Option<String>,
    /// `default`, `acceptEdits`, `bypassPermissions`, … as given.
    pub permission_mode: Option<String>,
    /// The CLI's own version. Reported, never compared against.
    pub cli_version: Option<String>,
    /// How many tools the agent was given. The names are on the event if ever needed;
    /// the count is what a strip can show.
    pub tools: usize,
    /// `(name, status)` per MCP server, in the order the init listed them.
    pub mcp_servers: Vec<(String, String)>,

    // -- from `result`, LATEST WINS ------------------------------------------
    /// 🚨 `total_cost_usd` is **cumulative across the session** already. Take the latest;
    /// adding two results together double-counts the whole session so far.
    pub cost_usd: Option<f64>,
    /// The tokens of the **most recent turn only** — the `result`'s `usage` sibling,
    /// which unlike the cost does *not* accumulate. Named so it cannot be read as a
    /// session total, because there is no honest way to derive one from it.
    pub last_turn_usage: Option<Usage>,
    /// Wall time of the most recent turn.
    pub last_turn_duration_ms: Option<u64>,

    // -- from `system/post_turn_summary`, LATEST WINS ------------------------
    /// A sentence describing the finished turn.
    pub last_status_detail: Option<String>,
    /// What the human is being asked for. `None` when the turn needs nothing.
    pub needs_action: Option<String>,
    /// e.g. `review_ready`.
    pub status_category: Option<String>,

    // -- from `rate_limit_event`, LATEST WINS --------------------------------
    /// `five_hour`, …
    pub rate_limit_type: Option<String>,
    /// Unix seconds at which the window resets.
    pub rate_limit_resets_at: Option<i64>,
    /// `allowed`, …
    pub rate_limit_status: Option<String>,
}

impl SessionFacts {
    /// Rule 3's other half. The caller guarantees this is the **first** init; a later one
    /// returns before reaching here, so identity cannot be overwritten mid-stream.
    fn record_init(&mut self, start: &SessionStart) {
        self.model = non_empty(&start.model);
        self.cwd = non_empty(&start.cwd);
        self.permission_mode = non_empty(&start.permission_mode);
        self.cli_version = non_empty(&start.cli_version);
        self.tools = start.tools.len();
        self.mcp_servers = start
            .mcp_servers
            .iter()
            .map(|server| (server.name.clone(), server.status.clone()))
            .collect();
    }

    /// Latest wins. Never sums — see the type docs on `cost_usd`.
    fn record_result(&mut self, result: &TurnResult) {
        if let Some(cost) = result.total_cost_usd {
            self.cost_usd = Some(cost);
        }
        if let Some(usage) = result.usage {
            self.last_turn_usage = Some(usage);
        }
        if let Some(duration) = result.duration_ms {
            self.last_turn_duration_ms = Some(duration);
        }
    }

    /// The three `post_turn_summary` fields are replaced **as a unit**, so a later turn
    /// that needs nothing *clears* an earlier turn's demand rather than leaving it
    /// standing — a stale "waiting on you" is worse than none. The test is on the fields
    /// rather than the subtype string, so a notice carrying none of them (`status`,
    /// `task_summary`) changes nothing.
    fn record_notice(&mut self, notice: &Notice) {
        let detail = notice.status_detail();
        let category = notice.status_category();
        // `needs_action` is empty on turns that need nothing; the accessor already
        // reports that as absence, so this must not re-filter it.
        let action = notice.needs_action();
        if detail.is_none() && category.is_none() && action.is_none() {
            return;
        }
        self.last_status_detail = detail.map(str::to_string);
        self.status_category = category.map(str::to_string);
        self.needs_action = action.map(str::to_string);
    }

    /// Latest wins.
    fn record_rate_limit(&mut self, limit: &RateLimit) {
        self.rate_limit_status = non_empty(&limit.status);
        if let Some(resets_at) = limit.resets_at {
            self.rate_limit_resets_at = Some(resets_at);
        }
        if let Some(limit_type) = &limit.limit_type {
            self.rate_limit_type = non_empty(limit_type);
        }
    }
}

/// An absent string field decodes to `""`. That is absence, not a fact worth showing.
fn non_empty(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// Decoder events in, transcript events out.
///
/// The state is small and all of it exists to satisfy rule 1: which message is
/// streaming, how many blocks of each message have settled, and which tool call each
/// open stream block belongs to.
#[derive(Debug, Default)]
pub struct EventMapper {
    /// Rule 3: the first `system/init` wins.
    session_started: bool,
    /// message id → content blocks settled so far, i.e. the next block's ordinal.
    settled_blocks: HashMap<String, usize>,
    /// The message id the `stream_event` lane is currently inside.
    streaming_message: Option<String>,
    /// Stream block index → tool call id, within the streaming message. Cleared on
    /// `message_start`, because indices restart with every message.
    streaming_tools: HashMap<usize, String>,
    /// Fallback identity for a message that arrived without an id. Monotonic so two
    /// anonymous messages never collide.
    anonymous: u64,
    stats: MapStats,
    facts: SessionFacts,
}

impl EventMapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> MapStats {
        self.stats
    }

    /// What the session has said about itself so far (rule 6). Read-only, like
    /// [`stats`](Self::stats): the mapper is the only thing that writes it.
    pub fn facts(&self) -> &SessionFacts {
        &self.facts
    }

    /// Map one decoded event. Returns the transcript events it becomes, in order —
    /// often none, sometimes several (one `assistant` line can carry text *and* a tool
    /// call, and one `user` line can carry several tool results).
    pub fn map(&mut self, event: &AgentEvent) -> Vec<cv::AgentEvent> {
        self.stats.events += 1;
        // Rule 5. Before anything else: a subagent's line must not touch the block
        // bookkeeping either, or its message ids would consume main-conversation
        // ordinals.
        if event.is_from_subagent() {
            self.stats.subagent_dropped += 1;
            return Vec::new();
        }
        match &event.kind {
            EventKind::SessionStarted(start) => {
                if self.session_started {
                    self.stats.repeat_session_starts += 1;
                    return Vec::new();
                }
                self.session_started = true;
                // After the guard, deliberately: rule 3 means the first init establishes
                // identity and a later one must not overwrite what it established.
                self.facts.record_init(start);
                let session_id = event.session_id.clone().unwrap_or_default();
                vec![cv::AgentEvent::SessionStarted { session_id }]
            }
            EventKind::Assistant(turn) => self.map_assistant(turn),
            EventKind::User(turn) => {
                let mut out = Vec::new();
                for result in &turn.tool_results {
                    out.push(cv::AgentEvent::ToolResult {
                        id: cv::ToolId::from(result.tool_use_id.as_str()),
                        output: result.text(),
                        is_error: result.is_error,
                    });
                }
                // Rule 2: replayed and genuine human text are the same thing here.
                let text = turn.human_text();
                if !text.is_empty() {
                    out.push(cv::AgentEvent::HumanInput { text });
                }
                if out.is_empty() {
                    self.stats.unmapped += 1;
                }
                out
            }
            EventKind::Stream(stream) => self.map_stream(stream),
            EventKind::Finished(result) => {
                self.facts.record_result(result);
                // Rule 4. `result`'s own text is the prose the assistant lines already
                // carried; the detail is what the view can say that it could not.
                let outcome =
                    if result.is_error { cv::RunOutcome::Error } else { cv::RunOutcome::Ok };
                let detail = [result.subtype.as_str()]
                    .into_iter()
                    .chain(result.terminal_reason.as_deref())
                    .find(|s| !s.is_empty())
                    .map(str::to_string);
                vec![cv::AgentEvent::RunFinished { outcome, detail }]
            }
            // These three render nothing into the flow and stay counted as `unmapped`
            // exactly as before — two of them now leave a *fact* behind on the way past,
            // which is a different question from whether anything was drawn.
            EventKind::Notice(notice) => {
                self.facts.record_notice(notice);
                self.stats.unmapped += 1;
                Vec::new()
            }
            EventKind::RateLimit(limit) => {
                self.facts.record_rate_limit(limit);
                self.stats.unmapped += 1;
                Vec::new()
            }
            EventKind::Unknown { .. } => {
                self.stats.unmapped += 1;
                Vec::new()
            }
        }
    }

    /// Convenience for tests and replays: map a whole slice.
    pub fn map_all<'a>(
        &mut self,
        events: impl IntoIterator<Item = &'a AgentEvent>,
    ) -> Vec<cv::AgentEvent> {
        events.into_iter().flat_map(|e| self.map(e)).collect()
    }

    /// One settled `assistant` line: one content block, in block order (rule 1).
    fn map_assistant(&mut self, turn: &crate::agent_event::AssistantTurn) -> Vec<cv::AgentEvent> {
        let message = match &turn.message_id {
            Some(id) => id.clone(),
            None => {
                self.anonymous += 1;
                format!("anon-{}", self.anonymous)
            }
        };
        let mut out = Vec::new();
        for block in &turn.content {
            // Consume an ordinal for EVERY block, rendered or not — see rule 1.
            let ordinal = self.settled_blocks.entry(message.clone()).or_insert(0);
            let key = block_key(&message, *ordinal);
            *ordinal += 1;
            match block {
                ContentBlock::Text(text) => out.push(cv::AgentEvent::AssistantMessage {
                    message: cv::MessageId(key),
                    text: text.clone(),
                }),
                ContentBlock::ToolUse(call) => out.push(cv::AgentEvent::ToolCall {
                    id: cv::ToolId::from(call.id.as_str()),
                    name: call.name.clone(),
                    // The authoritative input, re-serialised. `conversation` never
                    // parses arguments — the view does, and only once `complete`.
                    arguments: Some(call.input.to_string()),
                }),
                ContentBlock::Thinking { .. } | ContentBlock::Unknown { .. } => {
                    self.stats.unmapped += 1;
                }
            }
        }
        out
    }

    fn map_stream(&mut self, stream: &StreamEvent) -> Vec<cv::AgentEvent> {
        match stream {
            StreamEvent::MessageStart { message_id, .. } => {
                self.streaming_message = Some(match message_id {
                    Some(id) => id.clone(),
                    None => {
                        self.anonymous += 1;
                        format!("anon-{}", self.anonymous)
                    }
                });
                // Block indices restart with each message.
                self.streaming_tools.clear();
                Vec::new()
            }
            StreamEvent::BlockStart { index, block } => match block {
                // Opening the card here is what a live tool call looks like: the name is
                // known, the arguments are not yet, and the fragments that follow have
                // something to attach to.
                ContentBlock::ToolUse(call) => {
                    self.streaming_tools.insert(*index, call.id.clone());
                    vec![cv::AgentEvent::ToolCall {
                        id: cv::ToolId::from(call.id.as_str()),
                        name: call.name.clone(),
                        arguments: None,
                    }]
                }
                _ => Vec::new(),
            },
            StreamEvent::BlockDelta { index, delta } => match delta {
                Delta::Text(text) => {
                    let Some(message) = self.streaming_message.clone() else {
                        // A text delta with no message to attach it to. Not observed;
                        // dropping beats guessing, and the settled line still lands.
                        self.stats.unmapped += 1;
                        return Vec::new();
                    };
                    vec![cv::AgentEvent::AssistantDelta {
                        message: cv::MessageId(block_key(&message, *index)),
                        text: text.clone(),
                    }]
                }
                Delta::ToolInputJson(fragment) => match self.streaming_tools.get(index) {
                    Some(id) => vec![cv::AgentEvent::ToolArgumentsDelta {
                        id: cv::ToolId::from(id.as_str()),
                        fragment: fragment.clone(),
                    }],
                    None => {
                        self.stats.orphan_argument_fragments += 1;
                        Vec::new()
                    }
                },
                Delta::Thinking(_) | Delta::Signature(_) | Delta::Unknown { .. } => {
                    self.stats.unmapped += 1;
                    Vec::new()
                }
            },
            StreamEvent::BlockStop { .. }
            | StreamEvent::MessageDelta { .. }
            | StreamEvent::MessageStop => Vec::new(),
            StreamEvent::Unknown { .. } => {
                self.stats.unmapped += 1;
                Vec::new()
            }
        }
    }
}

/// The per-block key rule 1 turns on. `#` is safe: message ids are `msg_…`, and the
/// ordinal is the block's own index within that message.
fn block_key(message: &str, ordinal: usize) -> String {
    format!("{message}#{ordinal}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_event::{decode_all, decode_line};
    use crate::conversation::{Body, ToolState, Transcript};

    const TWO_TOOLS: &str = include_str!("../fixtures/claude_stream_two_tools.jsonl");
    const LIVE_SESSION: &str = include_str!("../fixtures/claude_stream_live_session.jsonl");
    const EDGES: &str = include_str!("../fixtures/claude_stream_edges.jsonl");

    /// Decode a fixture and fold it, exactly as the live pane does — the decode errors
    /// (the leading stdin warning, the `["not","an","object"]` line) are skipped here
    /// the same way the pane logs and continues.
    fn fold(text: &str) -> (Transcript, EventMapper) {
        let mut mapper = EventMapper::new();
        let mut transcript = Transcript::new();
        for outcome in decode_all(text) {
            let Ok(event) = outcome else { continue };
            for mapped in mapper.map(&event) {
                transcript.apply(mapped);
            }
        }
        (transcript, mapper)
    }

    fn texts(t: &Transcript) -> Vec<String> {
        t.elements()
            .iter()
            .filter_map(|e| e.assistant())
            .map(|a| a.text.clone())
            .collect()
    }

    /// 🚨 The trap this module exists for. Three `assistant` lines share one message id
    /// — prose, tool, tool — and a straight-through `message_id` would have the second
    /// line replace the first. The prose must survive, in place, ahead of both cards.
    #[test]
    fn prose_survives_two_tool_calls_that_share_its_message_id() {
        let (t, _) = fold(TWO_TOOLS);
        assert!(
            texts(&t).iter().any(|s| s == "I'll read both files."),
            "the first block's prose was lost to a same-id tool call: {:?}",
            texts(&t)
        );
        // …and the two cards are separate elements after it, not one replaced element.
        let bodies: Vec<&str> = t
            .elements()
            .iter()
            .map(|e| match &e.body {
                Body::Human(_) => "human",
                Body::Assistant(_) => "assistant",
                Body::Tool(_) => "tool",
                Body::RunEnd(_) => "end",
                Body::Artifact(_) => "artifact",
                Body::Approval(_) => "approval",
            })
            .collect();
        assert_eq!(
            bodies,
            vec!["assistant", "tool", "tool", "assistant", "end"],
            "arrival order, one element per block"
        );
    }

    /// Streaming and settled text land on the SAME element: the deltas accumulate, the
    /// settled line replaces. Two elements for one block would mean the keys disagree.
    #[test]
    fn streamed_and_settled_text_share_one_element() {
        let (t, _) = fold(TWO_TOOLS);
        let all = texts(&t);
        assert_eq!(all.len(), 2, "two assistant blocks in the capture, not four: {all:?}");
        assert_eq!(all[0], "I'll read both files.");
        assert!(all[1].starts_with("**fx-a.txt**"), "{:?}", all[1]);
        assert!(
            t.elements().iter().filter_map(|e| e.assistant()).all(|a| a.complete),
            "every block settled, so the deltas were replaced rather than appended to"
        );
    }

    /// Both tool cards resolve from their `tool_result` lines, keep their arguments, and
    /// nothing is left running once the turn is over.
    #[test]
    fn tool_cards_carry_name_arguments_and_output() {
        let (t, _) = fold(TWO_TOOLS);
        let cards: Vec<_> = t.elements().iter().filter_map(|e| e.tool()).collect();
        assert_eq!(cards.len(), 2);
        for card in &cards {
            assert_eq!(card.name.as_deref(), Some("Read"));
            assert!(card.arguments.complete, "the settled line carries the authoritative input");
            assert!(
                card.arguments.text.contains("file_path"),
                "arguments were not the tool's input: {:?}",
                card.arguments.text
            );
            assert!(matches!(card.state, ToolState::Complete { is_error: false, .. }));
        }
        assert!(cards[0].arguments.text.contains("fx-a.txt"));
        assert!(cards[1].arguments.text.contains("fx-b.txt"));
        assert!(!t.is_working(), "both results arrived");
    }

    /// The argument fragments really do stream: fed only the events up to the settled
    /// line, the card is running with partial, incomplete arguments. This is the state
    /// the view has to be able to draw.
    #[test]
    fn a_card_is_running_with_partial_arguments_before_its_settled_line() {
        let mut mapper = EventMapper::new();
        let mut t = Transcript::new();
        for outcome in decode_all(TWO_TOOLS) {
            let Ok(event) = outcome else { continue };
            // Stop just before the settled `assistant` line for the first tool call.
            let settled_tool = matches!(&event.kind, EventKind::Assistant(a)
                if a.tool_calls().next().is_some());
            if settled_tool {
                break;
            }
            for mapped in mapper.map(&event) {
                t.apply(mapped);
            }
        }
        let card = t.elements().iter().filter_map(|e| e.tool()).next().expect("a card");
        assert_eq!(card.name.as_deref(), Some("Read"), "the name is known at block start");
        assert!(card.state.is_running());
        assert!(!card.arguments.complete, "still streaming");
        assert!(
            card.arguments.text.contains("file_path"),
            "fragments were not accumulated: {:?}",
            card.arguments.text
        );
        assert!(t.is_working());
    }

    /// Rule 2 and rule 3 together, on the two-turn capture: both human turns appear
    /// (they came back on the stream), the second `system/init` did not reset anything,
    /// and there are two turns, not four.
    #[test]
    fn a_live_session_maps_two_turns_and_ignores_the_second_init() {
        let (t, m) = fold(LIVE_SESSION);
        let humans: Vec<String> =
            t.elements().iter().filter_map(|e| e.human()).map(|h| h.text.clone()).collect();
        assert_eq!(
            humans,
            vec!["Reply with exactly: TURN-ONE", "Reply with exactly: TURN-TWO"],
            "the replayed human turns are the transcript's only source of human text"
        );
        assert_eq!(m.stats().repeat_session_starts, 1, "the mid-stream init was dropped");
        assert_eq!(t.turns().len(), 2, "two turns: {:?}", t.turns());
        assert_eq!(texts(&t), vec!["TURN-ONE", "TURN-TWO"]);
        assert_eq!(
            t.session_id(),
            Some("22222222-2222-4222-8222-222222222222"),
            "identity comes from the first init"
        );
    }

    /// Rule 4: two `result` lines in one session are two turn ends, and neither closes
    /// the stream or duplicates the assistant's prose.
    #[test]
    fn each_result_is_a_turn_end_and_never_repeats_the_prose() {
        let (t, _) = fold(LIVE_SESSION);
        let ends = t.elements().iter().filter_map(|e| e.run_end()).count();
        assert_eq!(ends, 2, "one per turn");
        assert_eq!(
            texts(&t).iter().filter(|s| *s == "TURN-ONE").count(),
            1,
            "`result.result` must not be rendered as a second assistant block"
        );
    }

    /// Rule 5: the subagent-scoped assistant line in the edge fixture never reaches the
    /// transcript, and is counted rather than silently gone.
    #[test]
    fn subagent_lines_are_dropped_and_counted() {
        let (t, m) = fold(EDGES);
        assert_eq!(m.stats().subagent_dropped, 1);
        assert!(
            !texts(&t).iter().any(|s| s.contains("Searching the tree now")),
            "a subagent turn belonging to nobody must not appear: {:?}",
            texts(&t)
        );
    }

    /// The edge fixture's harder `user` shapes: a bare-string message, a mixed line
    /// (tool result + human aside + an image we do not render), and an error result
    /// whose content is an array of blocks rather than a string.
    #[test]
    fn user_lines_split_into_results_and_human_text() {
        let (t, _) = fold(EDGES);
        let humans: Vec<String> =
            t.elements().iter().filter_map(|e| e.human()).map(|h| h.text.clone()).collect();
        assert_eq!(
            humans,
            vec!["just a plain string, the stream-json INPUT form", "and here is a human aside"]
        );
        let cards: Vec<_> = t.elements().iter().filter_map(|e| e.tool()).collect();
        assert_eq!(cards.len(), 2, "two orphan results, kept rather than dropped");
        assert!(cards[0].name.is_none(), "an orphan card has no name to show");
        assert_eq!(cards[0].state.output(), Some("it worked"));
        assert!(cards[1].state.is_error(), "is_error must survive the mapping");
        assert_eq!(
            cards[1].state.output(),
            Some("File does not exist."),
            "the array form of a result is joined by the decoder, not lost"
        );
    }

    /// An unknown top-level type, an unknown stream event, an unknown block kind and a
    /// citations delta all render nothing — and all are counted, so "nothing arrived"
    /// and "we ignored it" stay different answers.
    #[test]
    fn unmapped_kinds_are_counted_not_silent() {
        let (_, m) = fold(EDGES);
        assert!(m.stats().unmapped >= 4, "counted: {:?}", m.stats());
        assert_eq!(m.stats().orphan_argument_fragments, 0);
    }

    /// The ordinal must be consumed by blocks this milestone renders nothing for, or
    /// every later block's key shifts and its deltas detach. A thinking block ahead of
    /// the prose is the cheapest case that proves it.
    #[test]
    fn an_unrendered_block_still_consumes_its_ordinal() {
        let lines = concat!(
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"msg_x","content":[{"type":"thinking","thinking":"weighing it up"}]}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"visible "}}}"#,
            "\n",
            r#"{"type":"assistant","message":{"id":"msg_x","content":[{"type":"text","text":"visible prose"}]}}"#,
            "\n",
        );
        let mut mapper = EventMapper::new();
        let mut t = Transcript::new();
        for outcome in decode_all(lines) {
            for mapped in mapper.map(&outcome.expect("valid json")) {
                t.apply(mapped);
            }
        }
        assert_eq!(
            texts(&t),
            vec!["visible prose"],
            "the settled text must replace the streamed fragment, not append beside it"
        );
        assert_eq!(t.elements().len(), 1, "one element for one block: {:?}", t.elements());
    }

    /// Two anonymous messages must not collide into one element.
    #[test]
    fn messages_without_an_id_get_distinct_keys() {
        let lines = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"one"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"two"}]}}"#,
            "\n",
        );
        let mut mapper = EventMapper::new();
        let mut t = Transcript::new();
        for outcome in decode_all(lines) {
            for mapped in mapper.map(&outcome.expect("valid json")) {
                t.apply(mapped);
            }
        }
        assert_eq!(texts(&t), vec!["one", "two"]);
    }

    // -- rule 6: the facts --------------------------------------------------

    /// A mapper that has seen nothing knows nothing. The distinction the whole type
    /// turns on is "not reported yet" vs "reported as empty", so the empty state must be
    /// `None` everywhere rather than a plausible-looking default.
    #[test]
    fn facts_start_empty() {
        let facts = EventMapper::new().facts().clone();
        assert_eq!(facts, SessionFacts::default());
        assert!(facts.model.is_none());
        assert!(facts.cwd.is_none());
        assert!(facts.permission_mode.is_none());
        assert!(facts.cli_version.is_none());
        assert!(facts.cost_usd.is_none());
        assert!(facts.last_turn_usage.is_none());
        assert!(facts.last_turn_duration_ms.is_none());
        assert!(facts.last_status_detail.is_none());
        assert!(facts.needs_action.is_none());
        assert!(facts.status_category.is_none());
        assert!(facts.rate_limit_status.is_none());
        assert_eq!(facts.tools, 0);
        assert!(facts.mcp_servers.is_empty());
    }

    /// The first `system/init`, read as reported. ⚠️ The model string keeps its `[1m]`
    /// suffix: that is what the CLI said, and trimming it to something prettier would be
    /// editorialising a measurement.
    #[test]
    fn the_first_init_is_recorded_verbatim() {
        let (_, m) = fold(LIVE_SESSION);
        let facts = m.facts();
        assert_eq!(
            facts.model.as_deref(),
            Some("claude-opus-5[1m]"),
            "the suffix is part of the reported model, not noise to strip"
        );
        assert_eq!(facts.cwd.as_deref(), Some(r"C:\work\demo"));
        assert_eq!(facts.permission_mode.as_deref(), Some("default"));
        assert_eq!(facts.cli_version.as_deref(), Some("2.1.228"));
        assert_eq!(facts.tools, 7, "the count, not the names");
        assert!(facts.mcp_servers.is_empty(), "the capture ran with none");
    }

    /// Rule 3 on the facts. The capture really does carry a second `system/init`
    /// (fixture line 7) and the mapper really does drop it — but the two agree field for
    /// field, so the capture alone cannot tell "did not overwrite" from "overwrote with
    /// identical values". So the captured line is replayed into the same mapper with its
    /// identity fields changed: an overwrite would now be visible.
    #[test]
    fn a_second_init_does_not_overwrite_the_first() {
        let (_, mut m) = fold(LIVE_SESSION);
        assert_eq!(m.stats().repeat_session_starts, 1, "the capture's own second init");
        let line = LIVE_SESSION
            .lines()
            .filter(|line| line.contains(r#""subtype":"init""#))
            .nth(1)
            .expect("the capture carries two inits")
            .replace("claude-opus-5[1m]", "a-different-model")
            .replace(r"C:\\work\\demo", r"C:\\elsewhere")
            .replace(r#""permissionMode":"default""#, r#""permissionMode":"bypassPermissions""#)
            .replace(r#""claude_code_version":"2.1.228""#, r#""claude_code_version":"9.9.9""#);
        let event = decode_line(&line).expect("the doctored init still decodes");
        assert!(m.map(&event).is_empty(), "a repeat init renders nothing");
        let facts = m.facts();
        assert_eq!(facts.model.as_deref(), Some("claude-opus-5[1m]"));
        assert_eq!(facts.cwd.as_deref(), Some(r"C:\work\demo"));
        assert_eq!(facts.permission_mode.as_deref(), Some("default"));
        assert_eq!(facts.cli_version.as_deref(), Some("2.1.228"));
        assert_eq!(m.stats().repeat_session_starts, 2, "and it was counted, not silent");
    }

    /// 🚨 `total_cost_usd` is cumulative on the wire — turn two's figure is turn one's
    /// plus its own. The latest is the session's cost; the sum is nearly double it.
    #[test]
    fn cost_is_the_latest_result_never_the_sum() {
        let (_, m) = fold(LIVE_SESSION);
        assert_eq!(
            m.facts().cost_usd,
            Some(0.0202),
            "0.0101 + 0.0202 = 0.0303 would be the session counted twice"
        );
    }

    /// The sibling `usage` is per turn, so it is held as the *last* turn's, named to say
    /// so. Turn two's cache reads are 52536 against turn one's 25282 — a total would be
    /// neither, and would double-count the cache besides.
    #[test]
    fn usage_and_duration_describe_the_last_turn_only() {
        let (_, m) = fold(LIVE_SESSION);
        let usage = m.facts().last_turn_usage.expect("two results arrived");
        assert_eq!(usage.cache_read_input_tokens, 52536, "turn two's, not a running total");
        assert_eq!(usage.cache_creation_input_tokens, 1128);
        assert_eq!(usage.output_tokens, 9);
        assert_eq!(m.facts().last_turn_duration_ms, Some(7389), "turn two's 7389ms");
    }

    /// The `post_turn_summary` fields, latest wins. `needs_action` is empty on both turns
    /// of the capture and must read as absent — a status strip saying "waiting on you"
    /// when nothing is wanted is worse than one saying nothing.
    #[test]
    fn the_post_turn_summary_supplies_the_latest_status() {
        let (_, m) = fold(LIVE_SESSION);
        let facts = m.facts();
        assert_eq!(
            facts.last_status_detail.as_deref(),
            Some("agent initializing for turn two"),
            "turn two's summary replaced turn one's"
        );
        assert_eq!(facts.status_category.as_deref(), Some("review_ready"));
        assert_eq!(facts.needs_action, None, "empty on the wire is absence");
    }

    /// A later turn that needs nothing must *clear* an earlier turn's demand rather than
    /// leave it standing — the three fields are replaced as a unit for exactly this.
    #[test]
    fn a_later_summary_clears_an_earlier_demand() {
        let lines = concat!(
            r#"{"type":"system","subtype":"post_turn_summary","status_category":"awaiting_input","status_detail":"asked a question","needs_action":"answer it"}"#,
            "\n",
            r#"{"type":"system","subtype":"post_turn_summary","status_category":"review_ready","status_detail":"done","needs_action":""}"#,
            "\n",
        );
        let mut m = EventMapper::new();
        for outcome in decode_all(lines) {
            m.map(&outcome.expect("valid json"));
        }
        assert_eq!(m.facts().needs_action, None, "the demand was answered and is gone");
        assert_eq!(m.facts().last_status_detail.as_deref(), Some("done"));
        assert_eq!(m.facts().status_category.as_deref(), Some("review_ready"));
    }

    /// `rate_limit_event` is the first line of every capture taken on this machine.
    /// Reported as given — a status and a reset time, and no percentage, because the
    /// wire carries neither a numerator nor a denominator to make one from.
    #[test]
    fn the_rate_limit_line_is_recorded_as_reported() {
        let (_, m) = fold(LIVE_SESSION);
        let facts = m.facts();
        assert_eq!(facts.rate_limit_status.as_deref(), Some("allowed"));
        assert_eq!(facts.rate_limit_type.as_deref(), Some("five_hour"));
        assert_eq!(facts.rate_limit_resets_at, Some(1786573800));
    }

    /// ⚠️ Reading a fact off a notice or a rate limit must not change what `unmapped`
    /// means: nothing is rendered into the flow for either, so both stay counted. The
    /// capture's three are its rate limit and its two `post_turn_summary` lines; the
    /// repeat init is counted separately and the rest all map.
    #[test]
    fn recording_facts_does_not_change_the_unmapped_count() {
        let (_, m) = fold(LIVE_SESSION);
        assert_eq!(m.stats().unmapped, 3, "{:?}", m.stats());
        assert_eq!(m.stats().repeat_session_starts, 1);
        assert_eq!(m.stats().subagent_dropped, 0);
    }
}

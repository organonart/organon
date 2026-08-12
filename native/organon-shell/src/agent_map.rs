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
//! # What is not mapped, and why that is not a gap
//!
//! `Notice` (including `post_turn_summary`), `RateLimit`, `tool_use_result`, thinking
//! blocks, and approvals are all held for milestone 2 by §5.9.3's closing note. They are
//! counted in [`MapStats::unmapped`], so "the view showed nothing" and "nothing arrived"
//! stay distinguishable.

use std::collections::HashMap;

use crate::agent_event::{AgentEvent, ContentBlock, Delta, EventKind, StreamEvent};
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
}

impl EventMapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> MapStats {
        self.stats
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
            EventKind::SessionStarted(_) => {
                if self.session_started {
                    self.stats.repeat_session_starts += 1;
                    return Vec::new();
                }
                self.session_started = true;
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
            EventKind::Notice(_) | EventKind::RateLimit(_) | EventKind::Unknown { .. } => {
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
    use crate::agent_event::decode_all;
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
}

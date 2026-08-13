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
//! identically.
//!
//! 🚨 **With exactly one exception, and rule 2 is why it exists.** Because the transcript
//! trusts the stream completely, a `user`-role line the console never sent becomes a
//! human turn with no further check — and the CLI emits one on every `set_model`,
//! narrating itself as
//! `<local-command-stdout>Set model to sonnet (claude-sonnet-5)</local-command-stdout>`
//! with the same `isReplay: true` the human's own echo carries. It arrives *before* the
//! ack, so waiting cannot suppress it. No
//! [`HumanInput`](crate::conversation::AgentEvent::HumanInput) is emitted for a line
//! [`UserTurn::local_command_output`](crate::agent_event::UserTurn::local_command_output)
//! recognises; that method owns the predicate and argues its own narrowness, because
//! swallowing a real turn is far worse than showing a spurious one. Tool results on such
//! a line are unaffected — only the human-text half is withheld.
//!
//! # 3. `system/init` recurs; the first establishes IDENTITY, a later one refreshes TWO fields
//!
//! A second `init` arrived mid-stream in the live-session capture — same `session_id`,
//! different field count — immediately before turn two. It still maps to nothing:
//! [`Transcript::apply`](crate::conversation::Transcript::apply) would open a fresh turn
//! for it and the human input that follows opens one anyway.
//!
//! 🚨 **But the fields split, and this amends the rule as originally written**
//! (`console_spike_execution_plan.md` §5.9.3 rule 3; James's ruling, recorded as a
//! proposal in `doc/console_session_control_protocol.md` §4b). A live `set_model`
//! genuinely changes the model, and **the only place the new value appears is a repeat
//! `system/init`** — measured `line 1 model=claude-opus-5[1m]` → `line 19
//! model=claude-sonnet-5`, same session id. First-init-wins for everything would leave
//! the status strip lying about the one fact it exists to report. So `model` and
//! `permissionMode` are **latest-wins**; identity is unchanged.
//!
//! ⚠️ **Adopting the whole later init would be wrong, and that is measured too.** Between
//! the same two inits `tools` went 33 → 128 and `mcp_servers` 0 → 4 with nothing asked
//! to change about either — MCP tools arrive *deferred*, so an init recurs simply because
//! more of them finished loading. A third session grew 102 → 131 with **no model change
//! at all**. An init is a restatement, not a change notification, so `tools`,
//! `mcp_servers`, `cwd` and `cli_version` stay first-init-wins. See
//! `SessionFacts::record_repeat_init`.
//!
//! # 4. `result` ends a TURN, not the stream
//!
//! It maps to [`RunFinished`](crate::conversation::AgentEvent::RunFinished), which
//! closes nothing — and its `result` text is deliberately **dropped**, because it is the
//! same prose the `assistant` lines already delivered. Rendering both would double every
//! final answer.
//!
//! # 5. Subagent-scoped events go to the card that spawned them
//!
//! The decoder distinguishes them ([`AgentScope::Subagent`], from `parent_tool_use_id`).
//! Rendered naively they appear as free-floating turns belonging to nobody, so milestone 1
//! dropped them outright and counted the loss. They are now routed instead: every
//! subagent-scoped line becomes a
//! [`SubagentActivity`](crate::conversation::AgentEvent::SubagentActivity) addressed to
//! the tool call named by its scope, and the transcript folds it onto that card
//! ([`crate::conversation`], behaviour 6).
//!
//! 🚨 **This changes what is rendered, not what arrives — and the difference is the whole
//! honesty of the feature.** §5.9.1 measured that Claude Code **never forwards
//! token-level deltas from a subagent**. So there is no live text here and there cannot
//! be: activity lands as complete bursts, sometimes minutes apart, and everything this
//! module emits for a subagent is a finished fact. Nothing may imply otherwise.
//!
//! ⚠️ **Rule 1 still holds and costs nothing to hold — for a measured reason, not a lucky
//! one.** A subagent's blocks never touch [`EventMapper::settled_blocks`] and are never
//! given a [`MessageId`](crate::conversation::MessageId) at all, so they cannot consume a
//! main-conversation ordinal. The per-block key exists to keep streamed deltas attached to
//! the settled text that replaces them; with no deltas forwarded there is nothing to
//! attach and nothing to detach. If subagent deltas ever *do* start arriving, that
//! reasoning collapses — which is why
//! [`MapStats::subagent_stream_events`] counts the event that would prove it.
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
//! **latest-wins** for everything that describes the most recent turn, and — the one
//! seam between them — **latest-init-wins** for the two fields a live control can
//! change. Nothing is summed. See [`SessionFacts`] for what is deliberately *not*
//! carried.
//!
//! 📌 The two changeable fields are **not symmetric**, and implementing them as though
//! they were is the trap. `set_permission_mode` emits a dedicated
//! `{"type":"system","subtype":"status","permissionMode":…}` alongside its ack — a
//! cheap, unambiguous subscription. `set_model` emits **no such event**: the new model
//! surfaces only in the repeat init above, in the next assistant message, and in the
//! `<local-command-stdout>` narration rule 2 suppresses. So the mode is read from both
//! its own event and a later init; the model has only the one source.
//!
//! # 7. A live turn state is not a fact, and only one of the two signals can carry it
//!
//! [`SessionFacts`] holds what the session *reported*: a field is set when a line carrying
//! it arrives and then holds that value until a later line replaces it. "The agent is
//! generating **right now**" is not that shape — it flips on and it flips off, and a
//! reader shown a stale one would be told the agent is working after it stopped. So it
//! lives beside the facts rather than on them, as [`EventMapper::is_generating`].
//!
//! Two candidate signals arrive, and 🚨 **they do not mean the same thing**:
//!
//! * **`system`/`status` = `"requesting"`** — a request is in flight. We are waiting on the
//!   API and no tokens have arrived.
//! * **`message_start` … `message_stop`** — tokens are actually arriving.
//!
//! **The bracket is what is reported**, and the reason is measured rather than a
//! preference. In `claude_stream_two_tools.jsonl` `"requesting"` appears **once**, ahead of
//! the first message, for a run that makes **two** API round trips — the second message
//! opens with no status line before it. And nothing anywhere says the request came *back*:
//! there is no `"responding"`, no closing status, no counterpart at all. A state keyed off
//! it would therefore be shown for a session's first request and silently absent for every
//! one after, with nothing to tell those two apart, and it would need a clearing rule
//! invented for it besides. The bracket has neither problem: it is emitted once per
//! message, and it closes itself.
//!
//! So a `requesting` notice is still read for facts and still renders nothing, exactly as
//! before. That is a refusal, not an oversight — and it is why this is one state and not
//! two: the honest second state would be blank most of the time it was true.
//!
//! ⚠️ **It must not stick on.** `message_stop` is the ordinary close and it is not the only
//! one. `result` clears it as well — a turn that errors out mid-message never reaches a
//! stop — and so does `system/init`, *including* a repeat one that rule 3 otherwise drops,
//! because an init arriving mid-stream means the message that was open is never going to
//! close. The one remaining exit is the process dying, and there is deliberately no clear
//! for it here: there is no event to clear on, because there is no stream. The view answers
//! that one with its own `Dead` reading, which outranks every other thing the band can say.
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
    AgentEvent, ContentBlock, Delta, EventKind, ModelUsage, Notice, RateLimit, SessionStart,
    StreamEvent, TurnResult, Usage,
};
use crate::conversation as cv;

/// What the mapper chose not to render, counted. Nothing here is an error; all of it is
/// the kind of thing that is invisible until someone asks why a card never appeared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapStats {
    /// Decoded lines seen, subagent lines included.
    pub events: u64,
    /// Subagent-scoped lines ([`AgentScope::Subagent`](crate::agent_event::AgentScope::Subagent))
    /// that produced at least one activity for a card.
    ///
    /// ⚠️ **This replaced `subagent_dropped`, and is not a rename.** That counter meant
    /// "how much we threw away"; this one means the opposite, so keeping the name while
    /// reversing the sense would have left a number whose every reader was wrong. Rule 5
    /// is the change.
    pub subagent_routed: u64,
    /// Subagent-scoped lines that produced nothing — a thinking block, an unknown shape,
    /// a subagent's own `system`/`result` line. The honest remainder of the old
    /// `subagent_dropped`, and the number that must be read next to
    /// [`subagent_routed`](Self::subagent_routed) rather than instead of it.
    pub subagent_unrendered: u64,
    /// 🚨 **The canary on §5.9.1's measurement.** Claude Code was measured not to forward
    /// `stream_event` lines from a subagent, and the whole design rests on it: no deltas
    /// means no live text to render, and rule 1's per-block key has nothing to key. This
    /// counts any that arrive anyway. **It should be zero forever**; if it is not, the
    /// measurement has changed and the subagent path needs designing again rather than
    /// patching.
    pub subagent_stream_events: u64,
    /// A `system/init` after the first (rule 3).
    pub repeat_session_starts: u64,
    /// Known event kinds this milestone renders nothing for: notices, rate limits,
    /// thinking, unknown blocks, unknown stream events.
    pub unmapped: u64,
    /// An `input_json_delta` whose block index named no open tool call. Never observed;
    /// counted so a stream shape change cannot hide.
    pub orphan_argument_fragments: u64,
    /// A `user` line that was the CLI narrating a local command rather than the human
    /// speaking (rule 2's exception), withheld from the transcript.
    ///
    /// ⚠️ Counted separately from [`unmapped`](Self::unmapped) on purpose: this is the
    /// one place the mapper *suppresses* something it fully understood, and the whole
    /// risk of doing so is that the predicate might one day match a real turn. A number
    /// that climbs while the user is typing is how that would be caught.
    pub local_commands_suppressed: u64,
    /// A `control_response` — an answer to something the console asked, which belongs to
    /// whatever holds the `request_id`, not to the transcript. Counted rather than
    /// folded into [`unmapped`](Self::unmapped): "the CLI answered us" and "we drew
    /// nothing for a stream event" are different facts.
    pub control_responses: u64,
}

/// What the session says about itself, as the stream says it (rule 6).
///
/// Everything here is *reported*, never computed: a field is `None` until a line
/// carrying it arrives, and it then holds that line's value verbatim. An empty string on
/// the wire is absence, not a fact, and is stored as `None`.
///
/// # What this deliberately does not carry
///
/// Two numbers a status strip obviously wants are missing, and each is missing because
/// the stream does not honestly carry it:
///
/// * **A quota percentage.** `rate_limit_event` carries a *status* and a reset time —
///   no numerator, no denominator anywhere.
/// * **A session token total.** Only `total_cost_usd` is cumulative on the wire. The
///   sibling `usage` is per turn, and summing it would double-count every cache read.
///   Cost is taken, tokens are not, and the field names say which is which.
///
/// ✏️ **A context-window percentage was on that list and has come off it, with the
/// numerator changed.** The refusal read: *"the denominator appears only inside the
/// unmodelled `modelUsage` block, per model, and the numerator would have to be a
/// running conversation size nothing on the wire reports."* Half of that was a gap and
/// half was a mistake. The denominator was never unavailable, only undecoded —
/// `modelUsage.contextWindow` is now [`ModelUsage::context_window`]. And the numerator
/// does not have to be a running total at all: **the last request's prompt is on the
/// wire, per request, on `message_start`.** So the reading is
/// [`ContextFill`] — the conversation as the model last saw it, over that model's
/// window, both measured — and the refusal survives in the shape of what is still
/// declined:
///
/// * 🚨 **Not `result.usage`.** It is summed across a turn's API round trips (the
///   `iterations` array is the proof), so on the two-request capture it reads **106 606**
///   against a largest true prompt of **54 050** — a bar that would show a session as
///   twice as full as it is, while looking exactly as confident. [`Usage::prompt_tokens`]
///   names both readings and says which object gives which.
/// * **Not a cumulative fill.** Nothing is summed here either. A conversation's context
///   goes *down* when the CLI compacts it, and only the next request reports that.
///
/// **`num_turns` is also declined**, for a different reason: it counts the turns of that
/// *run* and does not accumulate (it was `1` on both results of the two-turn capture), so
/// showing it as a session turn counter would show `1` forever. The view counts
/// `Transcript::turns()` instead, which it measured itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionFacts {
    // -- from `system/init`, LATEST INIT WINS (rule 3's amendment) -----------
    /// e.g. `claude-opus-5[1m]`. **Verbatim, suffix included** — that suffix is part of
    /// what the CLI reported, and trimming it would be editorialising a measurement.
    ///
    /// 🚨 **Latest-init-wins, unlike its neighbours.** A live `set_model` announces the
    /// new value nowhere but a repeat `system/init`, so first-init-wins here would pin
    /// the plate to a model the session stopped using.
    pub model: Option<String>,
    /// `default`, `acceptEdits`, `bypassPermissions`, … as given.
    ///
    /// Latest-wins from **two** sources: a later `system/init`, and the dedicated
    /// `system/status` line carrying `permissionMode` that `set_permission_mode` emits
    /// beside its ack. Either alone would be enough for the measured order; taking both
    /// means the strip is right whichever arrives.
    pub permission_mode: Option<String>,

    // -- from `system/init`, FIRST INIT WINS (rule 3) ------------------------
    /// The directory the agent operates in.
    pub cwd: Option<String>,
    /// The CLI's own version. Reported, never compared against.
    pub cli_version: Option<String>,
    /// How many tools the agent was given. The names are on the event if ever needed;
    /// the count is what a strip can show.
    ///
    /// ⚠️ **First-init-wins on purpose, even though a later init reports more.** The
    /// count grew 33 → 128 across two inits of one session purely because deferred MCP
    /// tools finished loading. Adopting the later figure would make the count *look*
    /// live while actually reporting loading progress.
    pub tools: usize,
    /// `(name, status)` per MCP server, in the order the init listed them. First-init
    /// -wins, for the same deferred-loading reason as [`tools`](Self::tools).
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
    /// The context window of the model the last request used, off `modelUsage`.
    ///
    /// A property of the *model*, not of the session, so latest-wins is a formality —
    /// two results for one model restate the same number. It changes on a model switch,
    /// which is exactly when latest-wins is the right rule. Chosen by name rather than
    /// by position: see [`context_window_for`].
    pub context_window: Option<u64>,

    // -- from `stream_event`/`message_start`, LATEST WINS --------------------
    /// The prompt the most recent API request carried — the conversation as the model
    /// last saw it, [`Usage::prompt_tokens`] of a `message_start`.
    ///
    /// 🚨 **Per request, and a turn makes several.** This moves mid-turn, once per round
    /// trip, which is the whole reason it is not read off the `result` — that line's
    /// `usage` is their sum. Latest-wins and **never summed**.
    ///
    /// ⚠️ It belongs here rather than beside `is_generating` even though its source is a
    /// stream event, because the retention rule is what decides that split (rule 7): this
    /// is a value the session *reported* and that stays true until the next request
    /// replaces it. A message closing does not make the last prompt size stale.
    pub last_prompt_tokens: Option<u64>,
    /// The model that request went to — `message_start`'s own `model` field, which is
    /// the **canonical** spelling (`claude-opus-5`) where `system/init` reports the
    /// variant one (`claude-opus-5[1m]`). Kept solely to pair the numerator above with
    /// the right window.
    pub last_prompt_model: Option<String>,

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
    /// Rule 3's other half. The caller guarantees this is the **first** init: it is the
    /// only one that establishes identity, and a later one reaches
    /// [`record_repeat_init`](Self::record_repeat_init) instead.
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

    /// 🚨 Rule 3's amendment: **exactly two fields follow a later init, and no others.**
    ///
    /// `model` and `permissionMode` are the two things a live control can change, and a
    /// repeat init is the only place `model` is ever restated. Everything else the init
    /// carries is deliberately left standing — `tools` and `mcp_servers` because their
    /// growth is deferred loading rather than change, `cwd` and `cli_version` because
    /// they are the transcript's identity and rule 3 still governs them in full.
    ///
    /// An empty string is absence, not a change: a later init that reports no model
    /// leaves the standing one alone rather than blanking the plate.
    fn record_repeat_init(&mut self, start: &SessionStart) {
        if let Some(model) = non_empty(&start.model) {
            self.model = Some(model);
        }
        if let Some(mode) = non_empty(&start.permission_mode) {
            self.permission_mode = Some(mode);
        }
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
        // ⚠️ The window only, and deliberately nothing else out of `modelUsage`: its
        // token and cost fields are session-cumulative restatements of what `cost_usd`
        // already carries, and a second writer for a number that has one is how two
        // readouts start disagreeing. A block that names no window leaves the standing
        // one alone rather than blanking the ring.
        if let Some(window) =
            context_window_for(self.last_prompt_model.as_deref(), &result.model_usage)
        {
            self.context_window = Some(window);
        }
    }

    /// One API request opening: the prompt it carried, and which model it went to.
    ///
    /// 🚨 **`message_start`'s usage is the one prompt size in the stream**, and this is
    /// the only place it is read. Nothing accumulates: the field is *assigned*, so the
    /// second request of a turn replaces the first rather than adding to it, which is
    /// the difference between this reading and the one `result.usage` would give.
    fn record_request(&mut self, model: Option<&String>, usage: Option<&Usage>) {
        if let Some(usage) = usage {
            self.last_prompt_tokens = Some(usage.prompt_tokens());
        }
        if let Some(model) = model.and_then(|m| non_empty(m)) {
            self.last_prompt_model = Some(model);
        }
    }

    /// The context reading, when both halves have been measured.
    ///
    /// `None` is a real answer and the common one at a cold start: no `result` has stated
    /// a window yet, or — on a session run without `--include-partial-messages`, which is
    /// what the `live_session` fixture captures — no `message_start` will ever state a
    /// prompt. There is deliberately no fallback for either half.
    pub fn context_fill(&self) -> Option<ContextFill> {
        let context_window = self.context_window.filter(|window| *window > 0)?;
        Some(ContextFill { prompt_tokens: self.last_prompt_tokens?, context_window })
    }

    /// The three `post_turn_summary` fields are replaced **as a unit**, so a later turn
    /// that needs nothing *clears* an earlier turn's demand rather than leaving it
    /// standing — a stale "waiting on you" is worse than none. The test is on the fields
    /// rather than the subtype string, so a notice carrying none of them (`status`,
    /// `task_summary`) changes nothing.
    fn record_notice(&mut self, notice: &Notice) {
        // 📌 The mode's own event source, and the reason the two changeable fields are
        // not implemented symmetrically. `set_permission_mode` emits
        // `{"type":"system","subtype":"status","status":null,"permissionMode":…}` beside
        // its ack — the same `system/status` shape whose other observed value is
        // `{"status":"requesting"}`, with `permissionMode` present only on the instance
        // that reports a change. Keyed on the *field*, not the subtype, so any line that
        // states the mode is heard and one that does not changes nothing.
        if let Some(mode) = notice.permission_mode().and_then(non_empty) {
            self.permission_mode = Some(mode);
        }
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

/// **Context at the last request** — the whole of what the console claims to know about
/// how full a conversation is.
///
/// Both halves are *measured*, and the name is the marker: this is not a running total,
/// not a session fill, and not a projection of where the turn will end up. It is the size
/// of the prompt the most recent API round trip carried, over the context window the
/// model that served it reports. A turn that makes three requests moves this three times.
///
/// 🚨 **The pairing is the correctness.** A numerator from one request and a denominator
/// from a different model would be a plausible-looking number with nothing behind it, so
/// [`context_window_for`] matches them by name and refuses when it cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextFill {
    /// [`Usage::prompt_tokens`] of the most recent `message_start`.
    pub prompt_tokens: u64,
    /// `modelUsage.contextWindow` for the model that request went to. Never zero — a
    /// zero window is treated as absence by [`SessionFacts::context_fill`], because a
    /// fraction over it is not a small reading, it is no reading.
    pub context_window: u64,
}

impl ContextFill {
    /// 0.0–1.0, **clamped at the top**.
    ///
    /// The clamp is for the drawing rather than for the truth: a prompt cannot really
    /// exceed the window it was accepted against, so a value over 1 would mean the two
    /// halves had been mispaired — and an arc that wrapped past its own start would hide
    /// exactly that. The raw counts are kept beside it and are what the hover states.
    pub fn fraction(&self) -> f32 {
        (self.prompt_tokens as f32 / self.context_window as f32).clamp(0.0, 1.0)
    }

    /// Whole percent, **floored**, for a reading a person can say out loud.
    ///
    /// 🚨 **Floored rather than rounded, and that is the honesty rule rather than a
    /// tiebreak: never report a fill that has not been reached.** A gauge that rounds
    /// *overstates* — at 74.6 % it would say 75, claiming a threshold the conversation
    /// has not crossed. This whole readout exists because the obvious numerator
    /// (`result.usage`) overstated the prompt by **1.97×**; a display that rounded its own
    /// answer up would reintroduce that same species of error at a tenth the scale.
    ///
    /// **Integer arithmetic, not [`fraction`](Self::fraction) floored** — and the reason is
    /// narrower than it looks, so it is stated rather than implied. Flooring the `f32` is
    /// *also* exact at every window worth having: both counts sit far below `2^24`, convert
    /// losslessly, and a sweep of 50 000 windows at an exact 75 % finds zero disagreement.
    /// The float is not wrong here; it is right **contingently**, on an input range nothing
    /// enforces. Past `2^24` the numerator stops being representable at all and an exact
    /// 99 % reads as 98 — understating the fill, which is the one direction this readout is
    /// not allowed to err in. Integer division is the floor by construction, so its
    /// correctness needs no argument about magnitudes; that matters more than usual now
    /// that [`ContextSlot::is_high`] takes the ring's colour from this number.
    /// [`fraction`](Self::fraction) stays `f32` because it draws an arc, where a fraction of
    /// a percent is a fraction of a pixel.
    ///
    /// The `min` mirrors that clamp for the same reason it exists there: a reading over
    /// 100 would mean the two halves had been mispaired. A zero window is absence, not a
    /// reading — [`SessionFacts::context_fill`] refuses to build one — and the guard is
    /// here so a hand-built `ContextFill` divides by zero nowhere.
    ///
    /// [`ContextSlot::is_high`]: crate::conversation_view::ContextSlot::is_high
    pub fn percent(&self) -> u64 {
        if self.context_window == 0 {
            return 0;
        }
        (self.prompt_tokens.saturating_mul(100) / self.context_window).min(100)
    }
}

/// Which `modelUsage` entry describes the model a request went to — **by name, never by
/// position**.
///
/// ⚠️ Two spellings have to be tried, and that is a measurement rather than defensiveness:
/// the block is keyed `claude-opus-5[1m]` while the `message_start` that names the model
/// says `claude-opus-5`, which is the entry's own `canonicalModel`. Matching only the key
/// would find nothing on the one capture that carries both.
///
/// **The single-entry fallback is the one inference here, and it is deliberate**: a turn
/// whose whole `modelUsage` block names one model used one model, so its window is not
/// ambiguous even when the identifiers do not line up (a gateway's fully-qualified id, a
/// spelling this build has not met). With two or more entries and no match there is a
/// real choice to make and nothing to make it with, so the answer is `None` and the ring
/// simply does not appear.
fn context_window_for(model: Option<&str>, entries: &[ModelUsage]) -> Option<u64> {
    if let Some(model) = model {
        let named = entries.iter().find(|entry| {
            entry.model == model || entry.canonical_model.as_deref() == Some(model)
        });
        if let Some(entry) = named {
            return entry.context_window;
        }
    }
    match entries {
        [only] => only.context_window,
        _ => None,
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
    /// Rule 7: is an assistant message open right now?
    ///
    /// ⚠️ **Deliberately a second field rather than `streaming_message.is_some()`**, even
    /// though the two look like one question. They are not: `streaming_message` is the *id
    /// deltas key against*, and it is kept after the message closes on purpose, so a text
    /// delta arriving late still lands on the block it belongs to instead of being dropped.
    /// This one answers "is it open", and it has to go false at `message_stop`. Making the
    /// id carry both meanings would trade a stuck status for lost text.
    generating: bool,
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

    /// Rule 7: **is an assistant message open — are tokens arriving right now?**
    ///
    /// Measured, not inferred. It is the `message_start` … `message_stop` bracket and
    /// nothing else: it is never derived from "a turn is open and no tools are running",
    /// which would be a guess wearing the same confident face as a measurement. Read-only,
    /// like [`stats`](Self::stats) and [`facts`](Self::facts) — the mapper is the only
    /// thing that writes it.
    ///
    /// ⚠️ **Not on [`SessionFacts`]**, and not a fact. See rule 7 for the retention
    /// argument and for every path that puts it back to `false`.
    pub fn is_generating(&self) -> bool {
        self.generating
    }

    /// Map one decoded event. Returns the transcript events it becomes, in order —
    /// often none, sometimes several (one `assistant` line can carry text *and* a tool
    /// call, and one `user` line can carry several tool results).
    pub fn map(&mut self, event: &AgentEvent) -> Vec<cv::AgentEvent> {
        self.stats.events += 1;
        // Rule 5. Before anything else, exactly as when this dropped the line: a
        // subagent's line must not touch the main block bookkeeping, or its message ids
        // would consume main-conversation ordinals.
        if let Some(parent) = event.subagent_tool_use_id() {
            let parent = cv::ToolId::from(parent);
            let out = self.map_subagent(&parent, &event.kind);
            if out.is_empty() {
                self.stats.subagent_unrendered += 1;
            } else {
                self.stats.subagent_routed += 1;
            }
            return out;
        }
        match &event.kind {
            EventKind::SessionStarted(start) => {
                // Rule 7, and **before the repeat guard on purpose**: an init arriving
                // mid-stream (rule 3 saw one) means whatever message was open is never
                // going to reach its `message_stop`. Dropping the line for the flow must
                // not also drop the only notice that the stream restarted.
                self.generating = false;
                if self.session_started {
                    self.stats.repeat_session_starts += 1;
                    // Rule 3's amendment, and the only thing that happens on this path
                    // besides the count: two fields follow the later init, the flow
                    // still gets nothing, and the counter still says one was dropped.
                    self.facts.record_repeat_init(start);
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
                // Rule 2's one exception: the CLI narrating one of its own local
                // commands is a `user` line the human never said. Counted as its own
                // thing rather than as `unmapped` — this is a line we recognised and
                // deliberately withheld, not one we had nothing to draw for. Only the
                // human-text half is withheld; tool results went out above.
                if turn.is_local_command_output() {
                    self.stats.local_commands_suppressed += 1;
                    return out;
                }
                // Rule 2 proper: replayed and genuine human text are the same thing.
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
                // Rule 7's abnormal close. A turn that fails part-way through a message
                // (`error_during_execution`, an interrupt) ends here and never reaches a
                // `message_stop`, so a status keyed only on the bracket would stay lit for
                // the rest of the session.
                self.generating = false;
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
            // An answer to a request the console itself wrote to stdin, correlated by a
            // `request_id` this module never issued and therefore cannot interpret. It
            // renders nothing into the flow and — deliberately — leaves **no fact**
            // behind either: the mode a `set_permission_mode` ack confirms arrives
            // independently as a `system/status` line, and reading both would be two
            // writers for one field where one of them cannot tell which verb it is
            // answering. Counted so a control the console sends is never simply gone.
            EventKind::ControlResponse(_) => {
                self.stats.control_responses += 1;
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

    /// Rule 5: one subagent-scoped line → activities for the card named by its scope.
    ///
    /// Deliberately a **separate, smaller** translation than the main-conversation path
    /// rather than a flag threaded through it. Only three shapes on the wire mean anything
    /// inside a card — the subagent said something, it ran a tool, a tool came back — and
    /// the main path's other concerns (the per-block key, the streaming lanes, the session
    /// facts, the generating bracket) are all either meaningless or actively wrong here.
    /// A shared path with a boolean would have had to remember which.
    ///
    /// 📌 What is declined, and why none of it is a gap:
    ///
    /// * **`stream_event`** — §5.9.1 measured these are never forwarded for a subagent.
    ///   Counted as the canary ([`MapStats::subagent_stream_events`]) rather than
    ///   speculatively handled, because handling an event that does not arrive is how a
    ///   view acquires a code path nobody has ever seen run.
    /// * **`system`/`init` and `result`** — a subagent's session bookkeeping is not the
    ///   console's session. Folding a subagent's `result` into
    ///   [`SessionFacts`] would let a subagent's cost overwrite the turn's.
    /// * **human text on a `user` line** — a subagent's prompt is the `Task` call's own
    ///   arguments, which the card already shows in full. Rendering it again inside the
    ///   card would be the same text twice.
    fn map_subagent(&mut self, parent: &cv::ToolId, kind: &EventKind) -> Vec<cv::AgentEvent> {
        use cv::Subagent;
        let mut out = Vec::new();
        match kind {
            EventKind::Assistant(turn) => {
                for block in &turn.content {
                    match block {
                        ContentBlock::Text(text) if !text.is_empty() => {
                            out.push(cv::AgentEvent::SubagentActivity {
                                parent: parent.clone(),
                                activity: Subagent::Said(text.clone()),
                            });
                        }
                        ContentBlock::ToolUse(call) => {
                            out.push(cv::AgentEvent::SubagentActivity {
                                parent: parent.clone(),
                                activity: Subagent::Used {
                                    id: cv::ToolId::from(call.id.as_str()),
                                    name: call.name.clone(),
                                },
                            });
                        }
                        _ => {}
                    }
                }
            }
            EventKind::User(turn) => {
                for result in &turn.tool_results {
                    out.push(cv::AgentEvent::SubagentActivity {
                        parent: parent.clone(),
                        activity: Subagent::Returned {
                            id: cv::ToolId::from(result.tool_use_id.as_str()),
                            is_error: result.is_error,
                        },
                    });
                }
            }
            // 🚨 The canary. If this ever fires, §5.9.1's measurement has changed.
            EventKind::Stream(_) => {
                self.stats.subagent_stream_events += 1;
            }
            _ => {}
        }
        out
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
            StreamEvent::MessageStart { message_id, model, usage } => {
                // Rule 6, and the one fact that arrives on a stream event rather than on
                // a `system` or `result` line: this request's prompt size. Recorded
                // before anything else here because it is true the moment the line lands,
                // whatever the message goes on to do.
                self.facts.record_request(model.as_ref(), usage.as_ref());
                self.streaming_message = Some(match message_id {
                    Some(id) => id.clone(),
                    None => {
                        self.anonymous += 1;
                        format!("anon-{}", self.anonymous)
                    }
                });
                // Block indices restart with each message.
                self.streaming_tools.clear();
                // Rule 7: tokens are about to arrive. Assigned rather than asserted, so a
                // message that opens while another is somehow still open is one open
                // message and not a count that could fail to reach zero.
                self.generating = true;
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
            // Rule 7's ordinary close. ⚠️ `streaming_message` is **not** cleared with it:
            // that id is what a late text delta keys against, and taking it away here would
            // trade a status bug for a lost sentence. See the field's own note.
            StreamEvent::MessageStop => {
                self.generating = false;
                Vec::new()
            }
            StreamEvent::BlockStop { .. } | StreamEvent::MessageDelta { .. } => Vec::new(),
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
    use crate::conversation::{Body, SubagentAct, SubagentLog, ToolState, Transcript};

    const TWO_TOOLS: &str = include_str!("../fixtures/claude_stream_two_tools.jsonl");
    const LIVE_SESSION: &str = include_str!("../fixtures/claude_stream_live_session.jsonl");
    const EDGES: &str = include_str!("../fixtures/claude_stream_edges.jsonl");
    /// 🚨 Hand-written, not captured — no fan-out has ever been recorded on this machine.
    /// `fixtures/README.md` states exactly which parts of it are reasoned rather than
    /// observed.
    const SUBAGENT: &str = include_str!("../fixtures/claude_stream_subagent.jsonl");

    /// Just the text a subagent produced, in order.
    fn said(log: &SubagentLog) -> Vec<String> {
        log.steps
            .iter()
            .filter_map(|s| match &s.act {
                SubagentAct::Said(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

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

    /// 🚨 CONTRACT — rule 5's whole point, and the half that did not change. A subagent's
    /// text must **never** become a top-level assistant block: that is the "turn belonging
    /// to nobody" the milestone-1 drop was protecting against, and routing the line
    /// somewhere better must not quietly reintroduce it.
    ///
    /// The edge fixture's subagent line names a parent that appears nowhere else in the
    /// file, so this also pins the orphan path — see the transcript-side test for what the
    /// card becomes.
    #[test]
    fn a_subagent_line_never_becomes_a_top_level_turn() {
        let (t, m) = fold(EDGES);
        assert!(
            !texts(&t).iter().any(|s| s.contains("Searching the tree now")),
            "a subagent turn belonging to nobody must not appear: {:?}",
            texts(&t)
        );
        assert_eq!(m.stats().subagent_routed, 1, "routed to a card, not dropped");
        assert_eq!(m.stats().subagent_unrendered, 0);
    }

    /// 🚨 CONTRACT — the correlation the whole feature turns on. The subagent's activity
    /// lands **inside** the `Task` card that spawned it, keyed by `parent_tool_use_id`, and
    /// appends no element of its own.
    #[test]
    fn a_subagents_work_lands_inside_the_task_card_that_spawned_it() {
        let (t, m) = fold(SUBAGENT);
        let cards: Vec<_> = t.elements().iter().filter_map(|e| e.tool()).collect();
        assert_eq!(
            cards.len(),
            1,
            "one Task call is one card; the subagent's own tools are steps, not elements: {:?}",
            cards.iter().map(|c| c.name.as_deref()).collect::<Vec<_>>()
        );
        let task = cards[0];
        assert_eq!(task.name.as_deref(), Some("Task"));
        assert!(!task.subagent.is_empty(), "the card carries the subagent's log");
        assert!(
            said(&task.subagent).iter().any(|s| s == "Searching the tree for ns_file."),
            "the subagent's text is on the card: {:?}",
            said(&task.subagent)
        );
        assert_eq!(m.stats().subagent_unrendered, 0, "{:?}", m.stats());
    }

    /// ⚠️ CONTRACT — the whole chain through the real decoder: a `Task` whose subagent
    /// dispatches its own `Task`. Both levels land on the **one** card the human can see,
    /// and the depth-2 work is labelled as such rather than passed off as direct.
    ///
    /// 📌 The fixture is hand-written — no fan-out has ever been captured on this machine
    /// — so what this proves is that the decoder's `parent_tool_use_id`, applied twice,
    /// resolves the way the model says. It does not prove a real subagent emits these
    /// lines in this order. `fixtures/README.md` says which is which.
    #[test]
    fn a_nested_dispatch_flattens_onto_the_one_visible_card() {
        let (t, _) = fold(SUBAGENT);
        let task = t.tool(&"toolu_0000000000000000000401".into()).expect("the Task card");
        assert_eq!(task.subagent.max_depth(), Some(2), "the inner dispatch was followed");
        assert!(
            said(&task.subagent).iter().any(|s| s == "Reading ipc.rs around line 41."),
            "the depth-2 agent's text is on the top-level card: {:?}",
            said(&task.subagent)
        );
        // The nested `Read` failed; its outcome survives two levels of flattening.
        assert!(
            task.subagent.steps.iter().any(|s| matches!(&s.act,
                SubagentAct::Tool { name: Some(n), state, .. } if n == "Read" && state.is_error())),
            "the depth-2 failure was lost: {:?}",
            task.subagent.steps
        );
        assert_eq!(
            t.elements().iter().filter_map(|e| e.tool()).count(),
            1,
            "four tool calls across two levels, one card"
        );
        assert_eq!(
            task.state.output(),
            Some("ns_file has three callers: ipc.rs:41, ipc.rs:88, mind_runtime.rs:12."),
            "and the card still resolves from its own result, as it always did"
        );
    }

    /// ⚠️ CONTRACT — §5.9.1's measurement, pinned as a *canary* rather than assumed.
    /// Claude Code does not forward token deltas from a subagent, which is why nothing in
    /// this path streams. The captures carry no such event, and if one ever appears the
    /// counter is how anyone finds out.
    #[test]
    fn no_capture_carries_a_stream_event_from_a_subagent() {
        for (name, text) in [
            ("two_tools", TWO_TOOLS),
            ("live_session", LIVE_SESSION),
            ("edges", EDGES),
            ("subagent", SUBAGENT),
        ] {
            let (_, m) = fold(text);
            assert_eq!(
                m.stats().subagent_stream_events,
                0,
                "{name}: §5.9.1 measured these are never forwarded — if this fires, the \
                 subagent path needs redesigning, not patching"
            );
        }
    }

    /// A subagent-scoped line the mapper renders nothing for is counted as unrendered, not
    /// as routed — the two numbers replaced one that said "dropped", and reading either
    /// alone would restate the lie the old counter told.
    #[test]
    fn a_subagent_line_with_nothing_to_render_counts_as_unrendered() {
        let thinking = concat!(
            r#"{"type":"assistant","message":{"id":"msg_s","content":[{"type":"thinking","thinking":"hmm"}]},"#,
            r#""parent_tool_use_id":"toolu_p"}"#,
            "\n",
        );
        let (t, m) = fold(thinking);
        assert!(t.elements().is_empty(), "nothing was drawn: {:?}", t.elements());
        assert_eq!(m.stats().subagent_unrendered, 1);
        assert_eq!(m.stats().subagent_routed, 0);
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
        // ⚠️ Three, not two: the fixture's subagent line names a parent that appears
        // nowhere else in it, so rule 5's routing opens a third orphan card for it. The
        // two this test is about are still the first two, in arrival order — the subagent
        // line is line 12, well after both results.
        assert_eq!(cards.len(), 3, "two orphan results and one orphan subagent parent");
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

    /// Rule 3 on the facts, **as amended**. The capture really does carry a second
    /// `system/init` (fixture line 7) and the two agree field for field, so the capture
    /// alone cannot tell "did not overwrite" from "overwrote with identical values". So
    /// the captured line is replayed into the same mapper with every field changed, and
    /// the split is then visible in one place: the transcript's identity — `cwd` and the
    /// CLI version — is held at what the first init said, while `model` and
    /// `permissionMode` follow the later one because a live control can change them and
    /// nothing else on the wire reports it.
    ///
    /// ⚠️ The line must still render nothing and must still be counted: the amendment is
    /// about which *facts* a repeat init refreshes, not about letting one back into the
    /// flow.
    #[test]
    fn a_second_init_does_not_overwrite_the_sessions_identity() {
        let (_, mut m) = fold(LIVE_SESSION);
        assert_eq!(m.stats().repeat_session_starts, 1, "the capture's own second init");
        let line = LIVE_SESSION
            .lines()
            .filter(|line| line.contains(r#""subtype":"init""#))
            .nth(1)
            .expect("the capture carries two inits")
            .replace("claude-opus-5[1m]", "a-different-model")
            .replace(r"C:\\work\\demo", r"C:\\elsewhere")
            .replace(r#""permissionMode":"default""#, r#""permissionMode":"acceptEdits""#)
            .replace(r#""claude_code_version":"2.1.228""#, r#""claude_code_version":"9.9.9""#);
        let event = decode_line(&line).expect("the doctored init still decodes");
        assert!(m.map(&event).is_empty(), "a repeat init renders nothing");
        let facts = m.facts();
        assert_eq!(
            facts.cwd.as_deref(),
            Some(r"C:\work\demo"),
            "identity is still the first init's, and a later one must not move it"
        );
        assert_eq!(facts.cli_version.as_deref(), Some("2.1.228"));
        assert_eq!(
            facts.model.as_deref(),
            Some("a-different-model"),
            "the amendment: a repeat init is the ONLY place a live model change appears"
        );
        assert_eq!(facts.permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(m.stats().repeat_session_starts, 2, "and it was counted, not silent");
    }

    /// One `system/init`, doctored per field. Enough of the shape to exercise the split
    /// and nothing more.
    fn init_line(model: &str, mode: &str, cwd: &str, version: &str, tools: &[&str], mcp: &[&str]) -> String {
        serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": "s-1",
            "cwd": cwd,
            "model": model,
            "permissionMode": mode,
            "claude_code_version": version,
            "tools": tools,
            "mcp_servers": mcp.iter().map(|name| serde_json::json!({"name": name, "status": "connected"})).collect::<Vec<_>>(),
        })
        .to_string()
    }

    fn map_lines(lines: &[String]) -> EventMapper {
        let mut m = EventMapper::new();
        for line in lines {
            m.map(&decode_line(line).expect("valid json"));
        }
        m
    }

    /// 🚨 CONTRACT — rule 3's amendment, forward direction. The measured sequence: line 1
    /// says `claude-opus-5[1m]`/`default`, a `set_model` and a `set_permission_mode` land
    /// mid-session, and line 19 of the same session id says `claude-sonnet-5`/
    /// `acceptEdits`. Without this the plate keeps the old model until the tab is closed
    /// — the strip lying about the one fact it exists to report.
    #[test]
    fn a_later_init_updates_the_model_and_the_permission_mode() {
        let m = map_lines(&[
            init_line("claude-opus-5[1m]", "default", r"C:\work", "2.1.228", &["Read"], &[]),
            init_line("claude-sonnet-5", "acceptEdits", r"C:\work", "2.1.228", &["Read"], &[]),
        ]);
        assert_eq!(m.facts().model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(m.facts().permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(m.stats().repeat_session_starts, 1, "still dropped from the flow, still counted");
    }

    /// 🚨 CONTRACT — the other direction, and the reason the whole later init must NOT be
    /// adopted. ⚠️ Between the same two inits of the measured session `tools` went 33 →
    /// 128 and `mcp_servers` 0 → 4 **with nothing asked to change about either**: MCP
    /// tools arrive deferred, so an init recurs simply because more of them finished
    /// loading. A third session grew 102 → 131 with no model change at all. So the split
    /// is field-wise, not line-wise — proved here by changing the model on the very same
    /// line that grows the roster.
    #[test]
    fn a_later_init_does_not_adopt_deferred_tools_the_mcp_roster_or_identity() {
        let m = map_lines(&[
            init_line("claude-opus-5[1m]", "default", r"C:\work", "2.1.228", &["Read", "Bash"], &[]),
            init_line(
                "claude-sonnet-5",
                "default",
                r"C:\somewhere-else",
                "9.9.9",
                &["Read", "Bash", "mcp__a__x", "mcp__a__y", "mcp__b__z"],
                &["a", "b"],
            ),
        ]);
        let facts = m.facts();
        assert_eq!(facts.model.as_deref(), Some("claude-sonnet-5"), "the one field that follows");
        assert_eq!(facts.tools, 2, "33 -> 128 was deferred loading, not a change to report");
        assert!(facts.mcp_servers.is_empty(), "the roster is the first init's");
        assert_eq!(facts.cwd.as_deref(), Some(r"C:\work"), "identity is rule 3's, unamended");
        assert_eq!(facts.cli_version.as_deref(), Some("2.1.228"));
    }

    /// An empty field on a later init is absence, not a change. A repeat init that omits
    /// the model must leave the standing one alone rather than blank the plate — the same
    /// rule the first init already follows.
    #[test]
    fn a_later_init_with_an_empty_field_does_not_blank_a_standing_fact() {
        let m = map_lines(&[
            init_line("claude-opus-5[1m]", "default", r"C:\work", "2.1.228", &["Read"], &[]),
            init_line("", "", r"C:\work", "2.1.228", &["Read"], &[]),
        ]);
        assert_eq!(m.facts().model.as_deref(), Some("claude-opus-5[1m]"));
        assert_eq!(m.facts().permission_mode.as_deref(), Some("default"));
    }

    /// ⚠️ The ordering the amendment must not disturb: `generating` is cleared **before**
    /// the repeat guard, because an init arriving mid-stream means the open message will
    /// never reach its `message_stop`. Recording facts on that path happens after, and
    /// changes nothing about it.
    #[test]
    fn a_later_init_still_clears_generating_before_it_records_anything() {
        let mut m = EventMapper::new();
        m.map(&decode_line(&init_line("claude-opus-5[1m]", "default", r"C:\w", "2.1.228", &[], &[])).unwrap());
        m.map(
            &decode_line(
                r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#,
            )
            .unwrap(),
        );
        assert!(m.is_generating(), "a message is open");
        let repeat = init_line("claude-sonnet-5", "acceptEdits", r"C:\w", "2.1.228", &[], &[]);
        assert!(m.map(&decode_line(&repeat).unwrap()).is_empty());
        assert!(!m.is_generating(), "the interrupted message will never close");
        assert_eq!(m.facts().model.as_deref(), Some("claude-sonnet-5"), "and the fact still landed");
    }

    // -- the permission mode's own event source ------------------------------

    /// 📌 CONTRACT: `set_permission_mode` emits a dedicated
    /// `{"type":"system","subtype":"status","permissionMode":…}` **in addition to** its
    /// ack, and the strip reads it. This is the asymmetry the protocol names: the mode
    /// has a clean event source and the model has none, so the mode is right the moment
    /// it changes rather than at the next init.
    #[test]
    fn a_status_line_carrying_a_permission_mode_updates_the_strip() {
        let m = map_lines(&[
            init_line("claude-opus-5[1m]", "default", r"C:\w", "2.1.228", &["Read"], &[]),
            r#"{"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits","session_id":"s-1"}"#.to_string(),
        ]);
        assert_eq!(m.facts().permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(m.facts().model.as_deref(), Some("claude-opus-5[1m]"), "it says nothing about the model");
        assert_eq!(m.facts().cwd.as_deref(), Some(r"C:\w"), "nor about identity");
        assert_eq!(m.stats().unmapped, 1, "it still renders nothing, and is still counted");
    }

    /// ⚠️ The ordinary `{"status":"requesting"}` line carries no `permissionMode` and must
    /// therefore leave the mode exactly as it stands. Keyed on the field, not the
    /// subtype, so a status line that says nothing about the mode changes nothing.
    #[test]
    fn a_requesting_status_does_not_disturb_the_permission_mode() {
        let m = map_lines(&[
            init_line("claude-opus-5[1m]", "acceptEdits", r"C:\w", "2.1.228", &[], &[]),
            r#"{"type":"system","subtype":"status","status":"requesting"}"#.to_string(),
        ]);
        assert_eq!(m.facts().permission_mode.as_deref(), Some("acceptEdits"));
    }

    // -- rule 2's exception: the fake human turn -----------------------------

    /// 🚨 CONTRACT: the `user`-role line `set_model` emits **must not become a human
    /// turn**. It arrives before the ack, carries `isReplay: true` exactly as a real
    /// human turn does, and decodes perfectly — so nothing but the wrapper stops the
    /// transcript acquiring a sentence the human never said.
    #[test]
    fn the_model_switch_narration_never_becomes_a_human_turn() {
        let lines = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"switch to sonnet please"}]},"isReplay":true}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model to sonnet (claude-sonnet-5)</local-command-stdout>"},"isReplay":true}"#,
            "\n",
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"req-model-1"}}"#,
            "\n",
        );
        let (t, m) = fold(lines);
        let humans: Vec<String> =
            t.elements().iter().filter_map(|e| e.human()).map(|h| h.text.clone()).collect();
        assert_eq!(
            humans,
            vec!["switch to sonnet please"],
            "the CLI's narration was rendered as something the human typed: {humans:?}"
        );
        assert_eq!(m.stats().local_commands_suppressed, 1, "withheld, and counted");
        assert_eq!(m.stats().control_responses, 1, "the ack is the console's, not the transcript's");
    }

    /// 🚨 CONTRACT — **the direction that matters more.** A human turn that quotes or
    /// discusses the wrapper is still a human turn. Swallowing a real message is far
    /// worse than showing a spurious one: the user watches their own sentence vanish with
    /// no way to tell what happened.
    #[test]
    fn a_human_turn_that_talks_about_the_wrapper_still_appears() {
        let mut lines = String::new();
        let said = [
            "Set model to sonnet (claude-sonnet-5)",
            "why does <local-command-stdout> render as me?",
            "it emits <local-command-stdout>Set model to sonnet</local-command-stdout> before the ack",
        ];
        for text in said {
            lines.push_str(
                &serde_json::json!({
                    "type": "user",
                    "message": {"role": "user", "content": [{"type": "text", "text": text}]},
                    "isReplay": true,
                })
                .to_string(),
            );
            lines.push('\n');
        }
        let (t, m) = fold(&lines);
        let humans: Vec<String> =
            t.elements().iter().filter_map(|e| e.human()).map(|h| h.text.clone()).collect();
        assert_eq!(humans, said.to_vec(), "a real human turn was eaten: {humans:?}");
        assert_eq!(m.stats().local_commands_suppressed, 0, "nothing should have matched");
    }

    /// A narration line that also carried a tool result would still deliver the result —
    /// only the human-text half is withheld. Never observed; pinned so a future shape
    /// cannot lose a tool's output to the suppression.
    #[test]
    fn suppressing_a_narration_does_not_drop_a_tool_result_beside_it() {
        // The decoder puts the wrapper in `text` and the result in `tool_results`; built
        // literally rather than hoping a capture ever produces one.
        let mixed = concat!(
            r#"{"type":"user","message":{"role":"user","content":["#,
            r#"{"type":"tool_result","tool_use_id":"toolu_1","content":"it worked"},"#,
            r#"{"type":"text","text":"<local-command-stdout>Set model to haiku</local-command-stdout>"}"#,
            r#"]},"isReplay":true}"#,
            "\n",
        );
        let (t, m) = fold(mixed);
        assert!(t.elements().iter().all(|e| e.human().is_none()), "no human turn");
        assert_eq!(
            t.elements().iter().filter_map(|e| e.tool()).count(),
            1,
            "the tool result must survive the suppression"
        );
        assert_eq!(m.stats().local_commands_suppressed, 1);
    }

    // -- the control protocol, at the seam -----------------------------------

    /// CONTRACT: a `control_response` renders nothing and leaves no fact. It is an answer
    /// to a request this module never issued, correlated by a `request_id` only the
    /// caller can interpret — and the mode it confirms arrives independently as a
    /// `system/status` line, so reading both would be two writers for one field.
    #[test]
    fn a_control_response_renders_nothing_and_records_no_fact() {
        let lines = concat!(
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"req-perm-1","response":{"mode":"acceptEdits"}}}"#,
            "\n",
            r#"{"type":"control_response","response":{"subtype":"error","request_id":"m-bypass","error":"Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions"}}"#,
            "\n",
        );
        let (t, m) = fold(lines);
        assert!(t.elements().is_empty(), "{:?}", t.elements());
        assert_eq!(m.facts(), &SessionFacts::default(), "no fact, not even the confirmed mode");
        assert_eq!(m.stats().control_responses, 2);
        assert_eq!(m.stats().unmapped, 0, "counted as answers, not as things we could not draw");
    }

    /// CONTRACT: an unrecognised control subtype is **counted, never fatal** — the same
    /// degrading discipline every other unknown on this stream gets. §5.9.3 rule 6 in
    /// spirit: the first line of a real run is not even JSON.
    #[test]
    fn an_unrecognised_control_subtype_is_counted_not_fatal() {
        let (t, m) = fold(
            "{\"type\":\"control_response\",\"response\":{\"subtype\":\"partial\",\"request_id\":\"r\",\"progress\":0.5}}\n",
        );
        assert!(t.elements().is_empty());
        assert_eq!(m.stats().control_responses, 1);
        assert_eq!(m.stats().events, 1, "it decoded, so it was seen");
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

    /// 🚨 CONTRACT — **the numerator is one request's prompt, never the turn's total.**
    /// This is the mistake the reading invites and the only thing standing between an
    /// honest ring and a confident wrong one, so both numbers are asserted here: the two
    /// requests of the capture's single turn, and the `result` that sums them.
    #[test]
    fn the_context_numerator_is_the_last_request_not_the_turns_total() {
        let (_, m) = fold(TWO_TOOLS);
        let facts = m.facts();
        assert_eq!(
            facts.last_prompt_tokens,
            Some(54_050),
            "the SECOND message_start's prompt — the conversation as the model last saw it"
        );
        let turn = facts.last_turn_usage.expect("the result arrived");
        assert_eq!(
            turn.prompt_tokens(),
            106_606,
            "the result's own usage, which is the two requests added together"
        );
        assert_ne!(
            facts.last_prompt_tokens,
            Some(turn.prompt_tokens()),
            "a ring built on the result would read 10% where the truth is 5%"
        );
    }

    /// The window comes off `modelUsage` and is paired with the model the request named.
    /// ⚠️ The two spellings differ — `message_start` says `claude-opus-5`, the block is
    /// keyed `claude-opus-5[1m]` — so matching the key alone would find nothing.
    #[test]
    fn the_window_is_matched_to_the_model_the_request_actually_used() {
        let (_, m) = fold(TWO_TOOLS);
        let facts = m.facts();
        assert_eq!(
            facts.last_prompt_model.as_deref(),
            Some("claude-opus-5"),
            "the canonical spelling, not the init one"
        );
        assert_eq!(facts.context_window, Some(1_000_000));
        let fill = facts.context_fill().expect("both halves measured");
        assert_eq!(fill.prompt_tokens, 54_050);
        assert_eq!(fill.percent(), 5, "54050 / 1000000");
    }

    /// A model whose entry is not in the block, alongside another that is, is not a
    /// window to guess at. One entry and no match is unambiguous; two are not.
    #[test]
    fn an_unmatched_model_takes_the_sole_window_and_refuses_a_choice() {
        let entry = |model: &str, window: u64| ModelUsage {
            model: model.to_string(),
            canonical_model: None,
            context_window: Some(window),
            ..Default::default()
        };
        let one = [entry("some-gateway/opus", 200_000)];
        let two = [entry("some-gateway/opus", 200_000), entry("haiku", 100_000)];
        assert_eq!(
            context_window_for(Some("unheard-of"), &one),
            Some(200_000),
            "one model served the turn, so its window is not ambiguous"
        );
        assert_eq!(
            context_window_for(Some("unheard-of"), &two),
            None,
            "two models and no match is a choice with nothing to make it with"
        );
        assert_eq!(context_window_for(Some("haiku"), &two), Some(100_000), "matched by key");
    }

    /// ⚠️ CONTRACT: a session that never states a per-request prompt gets **no reading**,
    /// not a reading off the `result`. `live_session` was captured without
    /// `--include-partial-messages`, so it carries a window and no `message_start` at all
    /// — which is exactly the shape that would tempt a fallback.
    #[test]
    fn a_window_without_a_prompt_size_is_no_context_reading_at_all() {
        let (_, m) = fold(LIVE_SESSION);
        let facts = m.facts();
        assert_eq!(facts.context_window, Some(1_000_000), "the window did arrive");
        assert_eq!(facts.last_prompt_tokens, None, "no message_start ever landed");
        assert!(
            facts.last_turn_usage.is_some(),
            "and the result's usage IS sitting there, unused — the fallback that must not exist"
        );
        assert_eq!(facts.context_fill(), None);
    }

    /// A zero window is absence rather than an infinitely full context: a fraction over
    /// it is not a small reading, it is no reading.
    #[test]
    fn a_zero_window_reports_nothing_rather_than_dividing_by_it() {
        let facts = SessionFacts {
            context_window: Some(0),
            last_prompt_tokens: Some(4_096),
            ..Default::default()
        };
        assert_eq!(facts.context_fill(), None);
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

    // -- rule 7: the live turn state ----------------------------------------

    /// One label per decoded line, for the lines this rule turns on. Everything else is
    /// `…`, so a trace reads as the sequence of milestones rather than as forty deltas.
    fn milestone(kind: &EventKind) -> String {
        match kind {
            EventKind::SessionStarted(_) => "init".to_string(),
            EventKind::Notice(notice) => format!("notice:{}", notice.subtype),
            EventKind::Finished(_) => "result".to_string(),
            EventKind::Stream(StreamEvent::MessageStart { .. }) => "message_start".to_string(),
            EventKind::Stream(StreamEvent::MessageStop) => "message_stop".to_string(),
            _ => "…".to_string(),
        }
    }

    /// What [`EventMapper::is_generating`] reads after each line of a capture.
    fn generating_trace(text: &str) -> Vec<(String, bool)> {
        let mut mapper = EventMapper::new();
        let mut trace = Vec::new();
        for outcome in decode_all(text) {
            let Ok(event) = outcome else { continue };
            let label = milestone(&event.kind);
            mapper.map(&event);
            trace.push((label, mapper.is_generating()));
        }
        trace
    }

    fn milestones(trace: &[(String, bool)]) -> Vec<(&str, bool)> {
        trace.iter().filter(|(l, _)| l != "…").map(|(l, g)| (l.as_str(), *g)).collect()
    }

    /// 🚨 **The distinction rule 7 exists for, on the capture that carries both signals.**
    ///
    /// `claude_stream_two_tools.jsonl` opens with `system/status` = `"requesting"` and then
    /// makes **two** message brackets. The trace has to show that the status line moved
    /// nothing — a request in flight is not tokens arriving — and that each bracket both
    /// set the state and put it back. If someone ever conflates the two, the second entry
    /// of this list turns `true` and this test says exactly which line did it.
    #[test]
    fn the_message_bracket_reports_generating_and_the_requesting_status_does_not() {
        let trace = generating_trace(TWO_TOOLS);
        assert_eq!(
            milestones(&trace),
            vec![
                ("init", false),
                // ⚠️ The load-bearing row. `"requesting"` means we are *waiting on* the API.
                ("notice:status", false),
                ("message_start", true),
                ("message_stop", false),
                ("notice:task_summary", false),
                ("message_start", true),
                ("message_stop", false),
                ("notice:post_turn_summary", false),
                ("result", false),
                ("notice:task_summary", false),
            ],
            "the full trace: {trace:?}"
        );
        // And it stays true for everything *inside* the bracket, not merely on the line that
        // opened it — the deltas, the tool blocks and the tool results all land in there.
        let opened = trace.iter().position(|(l, _)| l == "message_start").expect("a start");
        let closed = trace.iter().position(|(l, _)| l == "message_stop").expect("a stop");
        assert!(closed > opened + 1, "the capture carries content inside the bracket");
        assert!(
            trace[opened..closed].iter().all(|(_, generating)| *generating),
            "the state dropped somewhere inside an open message: {:?}",
            &trace[opened..closed]
        );
    }

    /// The `requesting` notice on its own, with nothing else: it must leave no state, no
    /// fact, and its `unmapped` count exactly as it was. Reading a line is not rendering it,
    /// and *declining* to read one is a decision this pins rather than an omission.
    #[test]
    fn a_requesting_status_alone_changes_nothing() {
        let line = r#"{"type":"system","subtype":"status","status":"requesting"}"#;
        let mut mapper = EventMapper::new();
        let event = decode_line(line).expect("a status notice decodes");
        assert!(mapper.map(&event).is_empty(), "it renders nothing into the flow");
        assert!(!mapper.is_generating(), "and it is NOT the signal that tokens are arriving");
        assert_eq!(mapper.facts(), &SessionFacts::default(), "it carries no summary fields");
        assert_eq!(mapper.stats().unmapped, 1, "counted, exactly as before");
    }

    /// ⚠️ **The abnormal ending.** A turn that fails part-way through a message never reaches
    /// its `message_stop` — `result` is the last thing the stream says. Without this clear the
    /// band would read "generating" for the rest of the session, which is worse than a band
    /// that never said it at all.
    #[test]
    fn a_result_clears_generating_when_no_message_stop_ever_arrives() {
        let lines = concat!(
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"half a sen"}}}"#,
            "\n",
            r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#,
            "\n",
        );
        let trace = generating_trace(lines);
        assert_eq!(
            milestones(&trace),
            vec![("message_start", true), ("result", false)],
            "{trace:?}"
        );
    }

    /// The other abnormal ending, and the one rule 3 nearly hides: a `system/init` arriving
    /// mid-stream. The live-session capture really carries one, and the mapper drops it for
    /// the flow — but dropping the line must not drop the fact that the stream restarted
    /// underneath an open message.
    #[test]
    fn a_mid_stream_init_clears_generating_even_though_it_is_dropped() {
        let init = LIVE_SESSION
            .lines()
            .find(|line| line.contains(r#""subtype":"init""#))
            .expect("the capture carries an init")
            .to_string();
        let start =
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#;
        let mut mapper = EventMapper::new();
        // The first init establishes identity; the message then opens under it.
        mapper.map(&decode_line(&init).expect("valid"));
        mapper.map(&decode_line(start).expect("valid"));
        assert!(mapper.is_generating(), "a message is open");
        // …and the repeat init arrives before it could ever close.
        let repeat = decode_line(&init).expect("valid");
        assert!(mapper.map(&repeat).is_empty(), "rule 3: a repeat init renders nothing");
        assert_eq!(mapper.stats().repeat_session_starts, 1, "and is counted, not silent");
        assert!(
            !mapper.is_generating(),
            "the message it interrupted will never reach a `message_stop`"
        );
    }

    /// The blunt end of every clearing path at once: **no capture leaves the mapper claiming
    /// the agent is still writing.** A new shape that opens a bracket and ends some way this
    /// module has not thought of fails here, on whichever fixture carries it.
    #[test]
    fn no_capture_ends_still_generating() {
        for (name, text) in
            [("two_tools", TWO_TOOLS), ("live_session", LIVE_SESSION), ("edges", EDGES)]
        {
            let (_, mapper) = fold(text);
            assert!(!mapper.is_generating(), "{name} ended with the state still lit");
        }
    }

    /// A mapper that has seen nothing is not generating, and a message that opens twice
    /// without closing is still one open message — the state is assigned, never counted, so
    /// there is no tally that could fail to reach zero.
    #[test]
    fn generating_starts_false_and_a_second_open_still_closes_once() {
        let mut mapper = EventMapper::new();
        assert!(!mapper.is_generating(), "nothing has arrived");
        let start =
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#;
        let stop = r#"{"type":"stream_event","event":{"type":"message_stop"}}"#;
        for _ in 0..2 {
            mapper.map(&decode_line(start).expect("valid"));
        }
        assert!(mapper.is_generating());
        mapper.map(&decode_line(stop).expect("valid"));
        assert!(!mapper.is_generating(), "one stop closes it, however many starts preceded it");
    }

    /// ⚠️ **The trade the second field buys.** `message_stop` must not clear
    /// `streaming_message`: a text delta arriving after the stop still has to key against
    /// the message it belongs to, or the console trades a stuck status indicator for a
    /// silently lost sentence. Pinned because the tempting simplification — asking
    /// `streaming_message.is_some()` and clearing it at the stop — breaks exactly this.
    #[test]
    fn closing_a_message_does_not_detach_a_late_delta_from_it() {
        let lines = concat!(
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_x"}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"before "}}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            "\n",
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"after"}}}"#,
            "\n",
        );
        let mut mapper = EventMapper::new();
        let mut t = Transcript::new();
        for outcome in decode_all(lines) {
            for mapped in mapper.map(&outcome.expect("valid json")) {
                t.apply(mapped);
            }
        }
        assert!(!mapper.is_generating(), "the bracket closed");
        assert_eq!(
            texts(&t),
            vec!["before after"],
            "the late fragment still found its block: {:?}",
            texts(&t)
        );
        assert_eq!(mapper.stats().unmapped, 0, "and nothing was dropped for want of an id");
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
        assert_eq!(m.stats().subagent_routed, 0, "this capture has no subagent in it");
    }
}
